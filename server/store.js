'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const Database = require('better-sqlite3');

const { World } = require('./world');
const storage = require('./storage');
const harness = require('./harness');

const dataDir = storage.DATA_DIR;
const databaseFile = path.join(dataDir, 'mcbench.sqlite3');
const worldsDir = path.join(dataDir, 'worlds');
const imagesDir = path.join(dataDir, 'images');
const attachmentsDir = path.join(dataDir, 'attachments');
const ARCHIVE_TTL_MS = 7 * 24 * 60 * 60 * 1000;
const MAX_ATTACHMENT_BYTES = 100 * 1024 * 1024;
const MAX_MESSAGE_ATTACHMENT_BYTES = 200 * 1024 * 1024;
const SCHEMA_VERSION = 13;
// Input the server refuses: the HTTP layer answers it 400, or `statusCode`.
const refused = (message, statusCode = 400) => Object.assign(new Error(message), { statusCode });

// Chats, attachments and worlds are the user's alone: other accounts on the
// machine cannot list or read them.
fs.mkdirSync(dataDir, { recursive: true });
fs.chmodSync(dataDir, 0o700);
// Startup removes every world folder no chat in the database names
// (collectChunks), so a new, empty database beside existing worlds would
// delete them all. A missing database with worlds beside it was lost or
// moved: the server stops before it creates one.
if (!fs.existsSync(databaseFile) && fs.existsSync(worldsDir) && fs.readdirSync(worldsDir).length > 0) {
  throw new Error(`The database ${databaseFile} is missing, but ${worldsDir} holds worlds. Restore the database first: starting without it would create an empty one and delete those worlds.`);
}
const database = new Database(databaseFile);
database.pragma('journal_mode = WAL');
database.pragma('synchronous = NORMAL');
database.pragma('foreign_keys = ON');
database.pragma('busy_timeout = 5000');
database.pragma('cache_size = -65536');
database.pragma('mmap_size = 268435456');
database.pragma('temp_store = MEMORY');

// Loaded worlds retain immutable chunk references across active checkpoints.
const live = new Map();

function chunkDir(id) {
  return path.join(worldsDir, id, 'chunks');
}

function chunkPath(id, hash) {
  return path.join(chunkDir(id), `${hash}.json`);
}

// Chunk files are named by their content, so a chunk is written once. Each
// new file counts against the chat's budget (the disk's reserve is checked at
// the first); `written` (file -> bytes) holds what this save added.
function chunkWriter(id, written) {
  return (blocks) => {
    const payload = JSON.stringify(blocks);
    const hash = crypto.createHash('sha256').update(payload).digest('hex').slice(0, 24);
    const file = chunkPath(id, hash);
    if (!fs.existsSync(file)) {
      // Pairs of integers: the JSON is ASCII, a byte a character.
      storage.reserve(id, payload.length, { disk: written.size === 0 });
      written.set(file, payload.length);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      const temporary = `${file}.${process.pid}.tmp`;
      fs.writeFileSync(temporary, payload);
      fs.renameSync(temporary, file);
    }
    return hash;
  };
}

// Undoes the chunk files a failed write added: removed, refunded, and no
// longer marked as persisted in `worlds`.
function discardChunks(id, written, worlds) {
  const hashes = new Set();
  for (const [file, bytes] of written) {
    hashes.add(path.basename(file, '.json'));
    fs.rmSync(file, { force: true });
    storage.refund(id, bytes);
  }
  for (const world of worlds) {
    for (const chunk of world.chunks.values()) {
      if (hashes.has(chunk.persistedHash)) chunk.persistedHash = null;
    }
  }
}

// Writes the chunk files of `world`, a build about to be applied, so a build
// that does not fit the chat's budget fails as the build tool's own result,
// and the save after it has no chunks left to write.
function persistWorld(conversationId, world) {
  const written = new Map();
  try {
    world.persistChunks(chunkWriter(conversationId, written));
  } catch (error) {
    discardChunks(conversationId, written, [world]);
    throw error;
  }
}

function chunkReader(id) {
  return (hash) => JSON.parse(fs.readFileSync(chunkPath(id, hash), 'utf8'));
}

function parseJson(value, label) {
  try {
    return JSON.parse(value);
  } catch (error) {
    throw new Error(`${label}: ${error.message}`);
  }
}

// Every folder of chat `id`: its world, images, attachments and /work.
function removeConversationArtifacts(id) {
  for (const folder of storage.chatFolders(id)) fs.rmSync(folder, { recursive: true, force: true });
}

// Full-text search: one document per title, bench round and user or
// assistant message, kept in an FTS5 index by triggers. `owner` is the chat
// or bench the document belongs to; `source` identifies it for updates.
const SEARCH_TABLES = `
  CREATE TABLE search_docs (
    id INTEGER PRIMARY KEY,
    owner TEXT NOT NULL,
    source TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL
  );
  CREATE VIRTUAL TABLE search USING fts5(body, content='search_docs', content_rowid='id', tokenize='porter unicode61');
  CREATE TRIGGER search_docs_insert AFTER INSERT ON search_docs BEGIN
    INSERT INTO search(rowid, body) VALUES (new.id, new.body);
  END;
  CREATE TRIGGER search_docs_delete AFTER DELETE ON search_docs BEGIN
    INSERT INTO search(search, rowid, body) VALUES ('delete', old.id, old.body);
  END;
  CREATE TRIGGER search_docs_update AFTER UPDATE ON search_docs BEGIN
    INSERT INTO search(search, rowid, body) VALUES ('delete', old.id, old.body);
    INSERT INTO search(rowid, body) VALUES (new.id, new.body);
  END;
`;

// What a message contributes to search: what the user asked and what the
// model said, never tool output or screenshots.
function messageText(payload) {
  if (payload.role === 'assistant') return typeof payload.content === 'string' ? payload.content : '';
  if (payload.role !== 'user' || payload.seeBuild) return '';
  return typeof payload.content === 'string'
    ? payload.content
    : payload.content.filter((part) => part.type === 'text').map((part) => part.text).join('\n');
}

function indexEverything() {
  const insert = database.prepare('INSERT INTO search_docs (owner, source, body) VALUES (?, ?, ?)');
  for (const row of database.prepare('SELECT id, title FROM conversations').all()) insert.run(row.id, `title:${row.id}`, row.title);
  for (const row of database.prepare('SELECT id, conversation_id, payload_json FROM messages').all()) {
    const text = messageText(JSON.parse(row.payload_json));
    if (text.trim()) insert.run(row.conversation_id, `message:${row.id}`, text);
  }
  for (const row of database.prepare('SELECT id, title, rounds_json FROM benches').all()) {
    insert.run(row.id, `title:${row.id}`, row.title);
    JSON.parse(row.rounds_json).forEach((round, index) => insert.run(row.id, `round:${row.id}:${index}`, messageText({ role: 'user', content: round.content })));
  }
}

const BENCHES_TABLE = `
  CREATE TABLE benches (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    rounds_json TEXT NOT NULL,
    archived_at INTEGER,
    head_round_id TEXT,
    blind INTEGER NOT NULL DEFAULT 0,
    revealed_at INTEGER,
    order_json TEXT NOT NULL DEFAULT '[]'
  );
`;

// One vote per version of a round: the model picked as best (NULL for a
// tie) among the models judged, and whether their names were hidden then.
const VOTES_TABLE = `
  CREATE TABLE bench_votes (
    round_id TEXT PRIMARY KEY,
    bench_id TEXT NOT NULL REFERENCES benches(id),
    pick TEXT,
    members_json TEXT NOT NULL,
    blind INTEGER NOT NULL,
    created_at INTEGER NOT NULL
  );
`;

// Lookups by parent message (the foreign-key check when messages are
// deleted), by bench and by search-document owner.
const LOOKUP_INDEXES = `
  CREATE INDEX messages_parent ON messages(parent_id);
  CREATE INDEX conversations_bench ON conversations(bench_id, bench_slot);
  CREATE INDEX bench_votes_bench ON bench_votes(bench_id);
  CREATE INDEX search_docs_owner ON search_docs(owner);
`;

// v7: a bench's rounds form a tree, edits branching like a chat's messages.
// Rounds get ids and parents, the bench a head, and each model's prompt the
// id of the round it answers.
function branchBenchRounds() {
  database.exec('DELETE FROM search_docs WHERE source LIKE \'round:%\'');
  const insert = database.prepare('INSERT INTO search_docs (owner, source, body) VALUES (?, ?, ?)');
  const setPayload = database.prepare('UPDATE messages SET payload_json = ? WHERE id = ?');
  for (const bench of database.prepare('SELECT id, rounds_json FROM benches').all()) {
    let parentId = null;
    const rounds = JSON.parse(bench.rounds_json).map((round) => {
      const node = { id: crypto.randomUUID(), parentId, content: round.content, createdAt: round.createdAt };
      parentId = node.id;
      insert.run(bench.id, `round:${bench.id}:${node.id}`, messageText({ role: 'user', content: round.content }));
      return node;
    });
    database.prepare('UPDATE benches SET rounds_json = ?, head_round_id = ? WHERE id = ?').run(JSON.stringify(rounds), parentId, bench.id);
    // Rounds so far are one line, so a prompt answers the round its depth names.
    for (const { id } of database.prepare('SELECT id FROM conversations WHERE bench_id = ?').all(bench.id)) {
      const rows = database.prepare('SELECT id, parent_id, payload_json FROM messages WHERE conversation_id = ? ORDER BY position').all(id);
      const depth = new Map();
      for (const row of rows) {
        const payload = JSON.parse(row.payload_json);
        const prompt = payload.role === 'user' && !payload.seeBuild;
        const own = (row.parent_id ? depth.get(row.parent_id) : 0) + (prompt ? 1 : 0);
        depth.set(row.id, own);
        if (prompt && rounds[own - 1]) setPayload.run(JSON.stringify({ ...payload, round: rounds[own - 1].id }), row.id);
      }
    }
  }
}

function initialize() {
  let version = database.pragma('user_version', { simple: true });
  if (version === SCHEMA_VERSION) return;
  if (version === 3) {
    // v4: when each chat's last turn finished and when the user last looked at it.
    database.transaction(() => {
      database.exec(`
        ALTER TABLE conversations ADD COLUMN finished_at INTEGER;
        ALTER TABLE conversations ADD COLUMN seen_at INTEGER;
      `);
      database.pragma('user_version = 4');
    })();
    version = 4;
  }
  if (version === 4) {
    // v5: benches, whose models each run as an ordinary chat in the bench.
    database.transaction(() => {
      database.exec(`
        ${BENCHES_TABLE}
        ALTER TABLE conversations ADD COLUMN bench_id TEXT REFERENCES benches(id);
        ALTER TABLE conversations ADD COLUMN bench_slot INTEGER;
      `);
      database.pragma('user_version = 5');
    })();
    version = 5;
  }
  if (version === 5) {
    // v6: full-text search over titles, rounds and messages.
    database.transaction(() => {
      database.exec(SEARCH_TABLES);
      indexEverything();
      database.pragma('user_version = 6');
    })();
    version = 6;
  }
  if (version === 6) {
    database.transaction(() => {
      database.exec('ALTER TABLE benches ADD COLUMN head_round_id TEXT');
      branchBenchRounds();
      database.pragma('user_version = 7');
    })();
    version = 7;
  }
  if (version === 7) {
    // v8: a selection names its main block, which its icon shows. Labels
    // start with that block's name ("Dark oak stairs, top"), so older
    // selections get it back from there.
    database.transaction(() => {
      const setPayload = database.prepare('UPDATE messages SET payload_json = ? WHERE id = ?');
      for (const row of database.prepare(`SELECT id, payload_json FROM messages WHERE payload_json LIKE '%"type":"selection"%'`).all()) {
        const payload = JSON.parse(row.payload_json);
        payload.content = payload.content.map((part) => (part.type !== 'selection' ? part : {
          ...part,
          selection: {
            ...part.selection,
            block: part.selection.label === 'Empty space' ? null : `minecraft:${part.selection.label.split(',')[0].toLowerCase().replaceAll(' ', '_')}`,
          },
        }));
        setPayload.run(JSON.stringify(payload), row.id);
      }
      database.pragma('user_version = 8');
    })();
    version = 8;
  }
  if (version === 8) {
    // v9: blind benches and votes. A bench lists its models in the order
    // their letters run; existing ones keep the order they were added in.
    database.transaction(() => {
      database.exec(`
        ALTER TABLE benches ADD COLUMN blind INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE benches ADD COLUMN revealed_at INTEGER;
        ALTER TABLE benches ADD COLUMN order_json TEXT NOT NULL DEFAULT '[]';
        ${VOTES_TABLE}
      `);
      for (const { id } of database.prepare('SELECT id FROM benches').all()) {
        const members = database.prepare('SELECT id FROM conversations WHERE bench_id = ? ORDER BY bench_slot').all(id).map((row) => row.id);
        database.prepare('UPDATE benches SET order_json = ? WHERE id = ?').run(JSON.stringify(members), id);
      }
      database.pragma('user_version = 9');
    })();
    version = 9;
  }
  if (version === 9) {
    // v10: lookup indexes, and free pages handed back to the filesystem
    // (a one-time rebuild switches the file to incremental vacuum).
    database.transaction(() => {
      database.exec(LOOKUP_INDEXES);
      database.pragma('user_version = 10');
    })();
    database.pragma('auto_vacuum = INCREMENTAL');
    database.exec('VACUUM');
    version = 10;
  }
  if (version === 10) {
    // v11: extra mode, per chat (replaced by resolution in v12).
    database.transaction(() => {
      database.exec('ALTER TABLE conversations ADD COLUMN extra INTEGER NOT NULL DEFAULT 0');
      database.pragma('user_version = 11');
    })();
    version = 11;
  }
  if (version === 11) {
    // v12: a chat's build resolution (resolution.js) replaces extra mode,
    // which was the highest stop.
    database.transaction(() => {
      database.exec(`
        ALTER TABLE conversations ADD COLUMN resolution INTEGER NOT NULL DEFAULT 1;
        UPDATE conversations SET resolution = 10 WHERE extra = 1;
        ALTER TABLE conversations DROP COLUMN extra;
      `);
      database.pragma('user_version = 12');
    })();
    version = 12;
  }
  if (version === 12) {
    // v13: Claude Code's context readings before this version were the
    // turn's summed input, not the last request's: they are dropped.
    database.transaction(() => {
      database.exec(`
        UPDATE messages SET payload_json = json_remove(payload_json, '$.telemetry.contextTokens')
        WHERE json_extract(payload_json, '$.telemetry.provider') = 'claude-code'
          AND json_type(payload_json, '$.telemetry.contextTokens') IS NOT NULL;
      `);
      database.pragma(`user_version = ${SCHEMA_VERSION}`);
    })();
    return;
  }
  if (version !== 0) throw new Error(`Database schema version ${version} is unsupported (expected ${SCHEMA_VERSION})`);
  // The WAL header is already written, so the setting takes a rebuild.
  database.pragma('auto_vacuum = INCREMENTAL');
  database.exec('VACUUM');
  database.transaction(() => {
    database.exec(`
      ${BENCHES_TABLE}
      ${VOTES_TABLE}

      CREATE TABLE conversations (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        effort TEXT,
        resolution INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        current_message_id TEXT,
        world_manifest_json TEXT NOT NULL,
        archived_at INTEGER,
        finished_at INTEGER,
        seen_at INTEGER,
        bench_id TEXT REFERENCES benches(id),
        bench_slot INTEGER
      );

      CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        parent_id TEXT REFERENCES messages(id),
        position INTEGER NOT NULL CHECK (position >= 0),
        payload_json TEXT NOT NULL,
        checkpoint_manifest_json TEXT,
        world_manifest_json TEXT,
        UNIQUE (conversation_id, position)
      );

      CREATE INDEX conversations_active_created
        ON conversations(archived_at, created_at DESC);

      ${SEARCH_TABLES}
      ${LOOKUP_INDEXES}
    `);
    database.pragma(`user_version = ${SCHEMA_VERSION}`);
  })();
}

initialize();
database.pragma('incremental_vacuum');

// Chunk files no stored world names any more (left by edits, branches and
// reverts) are removed at startup, as are folders of deleted chats.
// storage.expendable decides what goes, here and for test_build reports.
function collectChunks() {
  // Most messages repeat the world before them: each distinct manifest is read once.
  const manifests = database.prepare(`
    SELECT id AS owner, world_manifest_json AS manifest FROM conversations
    UNION SELECT conversation_id, world_manifest_json FROM messages WHERE world_manifest_json IS NOT NULL
    UNION SELECT conversation_id, checkpoint_manifest_json FROM messages WHERE checkpoint_manifest_json IS NOT NULL
  `).all();
  const referenced = new Set();
  for (const { owner, manifest } of manifests) {
    for (const chunk of JSON.parse(manifest).chunks) referenced.add(`${owner}/${chunk[1]}`);
  }
  const conversations = new Set(database.prepare('SELECT id FROM conversations').pluck().all());
  if (!fs.existsSync(worldsDir)) return;
  for (const id of fs.readdirSync(worldsDir)) {
    if (!conversations.has(id)) {
      fs.rmSync(path.join(worldsDir, id), { recursive: true, force: true });
      continue;
    }
    const directory = chunkDir(id);
    if (!fs.existsSync(directory)) continue;
    const names = fs.readdirSync(directory);
    const named = new Set(names.filter((name) => referenced.has(`${id}/${path.basename(name, '.json')}`)));
    for (const name of storage.expendable(names, named, 0)) fs.rmSync(path.join(directory, name), { force: true });
  }
}
collectChunks();

// The test_build reports (`<uuid>.json` in /work/verification) that chat
// `id`'s stored messages name, which are never pruned.
const REPORT_PATH = /\/work\/verification\/([0-9a-f-]{36}\.json)/g;
function reportReferences(id) {
  const names = new Set();
  const payloads = database.prepare(`
    SELECT payload_json FROM messages WHERE conversation_id = ? AND instr(payload_json, '/work/verification/') > 0
  `).pluck().all(id);
  for (const payload of payloads) {
    for (const match of payload.matchAll(REPORT_PATH)) names.add(match[1]);
  }
  return names;
}

const indexDoc = database.prepare(`
  INSERT INTO search_docs (owner, source, body) VALUES (?, ?, ?)
  ON CONFLICT(source) DO UPDATE SET body = excluded.body WHERE body != excluded.body
`);

// Images are stored as files and referenced by URL from messages; base64
// payloads never enter SQLite. Each of `images` is a Buffer (png) or a data
// URL; together they count against the chat's budget before any is written.
function saveImages(conversationId, images, prefix = '') {
  const decoded = images.map((data) => {
    if (typeof data !== 'string') return { buf: data, ext: 'png' };
    const match = data.match(/^data:image\/(\w+);base64,/);
    if (!match) throw new Error('Expected an image data URL');
    return { buf: Buffer.from(data.slice(match[0].length), 'base64'), ext: match[1] === 'jpeg' ? 'jpg' : match[1] };
  });
  storage.reserve(conversationId, decoded.reduce((sum, image) => sum + image.buf.length, 0));
  const dir = path.join(imagesDir, conversationId);
  fs.mkdirSync(dir, { recursive: true });
  return decoded.map(({ buf, ext }) => {
    const name = `${prefix}${crypto.randomUUID().slice(0, 8)}.${ext}`;
    fs.writeFileSync(path.join(dir, name), buf);
    return `/api/images/${conversationId}/${name}`;
  });
}

function imageFile(url) {
  const parts = url.split('/');
  const file = path.normalize(path.join(imagesDir, parts[3], parts[4]));
  if (!file.startsWith(imagesDir)) throw new Error('Bad image path');
  return file;
}

function readImage(url) {
  return fs.readFileSync(imageFile(url));
}

function decodeAttachment(file) {
  const originalName = path.basename(String(file?.name || '')).replace(/[\u0000-\u001f\u007f]/g, '').slice(0, 160);
  if (!originalName) throw refused('Attachment name is required');
  const data = String(file?.data || '');
  const match = data.match(/^data:([^;,]+);base64,/);
  if (!match) throw refused(`Attachment "${originalName}" is not base64 data`);
  const buffer = Buffer.from(data.slice(match[0].length), 'base64');
  if (buffer.length === 0) throw refused(`Attachment "${originalName}" is empty`);
  if (buffer.length > MAX_ATTACHMENT_BYTES) throw refused(`Attachment "${originalName}" exceeds 100 MB`);
  return { originalName, type: match[1], buffer };
}

// A message's attachments are all decoded and checked (their sizes, then the
// chat's budget and the disk's reserve) before any is written, so a refused
// message leaves no files behind.
function saveAttachments(conversationId, files) {
  const decoded = files.map(decodeAttachment);
  const total = decoded.reduce((sum, file) => sum + file.buffer.length, 0);
  if (total > MAX_MESSAGE_ATTACHMENT_BYTES) {
    throw refused('Attachments exceed 200 MB total');
  }
  const dir = path.join(attachmentsDir, conversationId);
  const saved = decoded.map(({ originalName, type, buffer }) => {
    const mimeExtension = {
      'image/jpeg': '.jpg',
      'image/png': '.png',
      'image/webp': '.webp',
      'image/gif': '.gif',
    }[type];
    const extension = mimeExtension || path.extname(originalName).replace(/[^a-zA-Z0-9.]/g, '').slice(0, 16);
    const stem = path.basename(originalName, path.extname(originalName))
      .replace(/[^a-zA-Z0-9._-]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 80) || 'file';
    const storedName = `${crypto.randomUUID().slice(0, 8)}-${stem}${extension}`;
    return {
      name: originalName,
      type,
      size: buffer.length,
      url: `/api/attachments/${conversationId}/${storedName}`,
      path: `/work/attachments/${storedName}`,
    };
  });
  storage.reserve(conversationId, total);
  try {
    fs.mkdirSync(dir, { recursive: true });
    saved.forEach((file, i) => fs.writeFileSync(attachmentFile(file.url), decoded[i].buffer));
  } catch (error) {
    discardAttachments(conversationId, saved);
    throw error;
  }
  return saved;
}

// Undoes saveAttachments: the files are written before the row that names
// them, so when that row is not saved they are removed and their bytes given
// back, and a folder left empty goes with them.
function discardAttachments(conversationId, saved) {
  for (const file of saved) {
    fs.rmSync(attachmentFile(file.url), { force: true });
    storage.refund(conversationId, file.size);
  }
  const dir = path.join(attachmentsDir, conversationId);
  if (fs.existsSync(dir) && fs.readdirSync(dir).length === 0) fs.rmdirSync(dir);
}

const saveAttachment = (conversationId, file) => saveAttachments(conversationId, [file])[0];

function attachmentFile(url) {
  const parts = url.split('/');
  const file = path.resolve(attachmentsDir, parts[3] || '', parts[4] || '');
  if (!file.startsWith(`${attachmentsDir}${path.sep}`)) throw new Error('Bad attachment path');
  return file;
}

function readAttachment(url) {
  return fs.readFileSync(attachmentFile(url));
}

function messagePayload(message) {
  const { id, parentId, position, ...payload } = message;
  return payload;
}

function publicMessages(conversation) {
  return conversation.messages.map((message) => {
    const { parentId, position, ...publicMessage } = message;
    return publicMessage;
  });
}

function appendMessage(conversation, payload, checkpoint = null) {
  const message = {
    ...payload,
    createdAt: payload.createdAt || new Date().toISOString(),
    id: crypto.randomUUID(),
    parentId: conversation.currentMessageId || null,
    position: conversation.nextMessagePosition,
  };
  conversation.nextMessagePosition += 1;
  conversation.currentMessageId = message.id;
  conversation.messages.push(message);
  conversation.unsaved.add(message.id);
  if (checkpoint) conversation.checkpoints.set(message.id, checkpoint);
  return message;
}

function dismissError(conversation, messageId) {
  const message = conversation.messages.find((entry) => entry.id === messageId);
  if (!message || message.role !== 'error') throw refused('messageId must point at an error message');
  message.dismissed = true;
  conversation.unsaved.add(message.id);
  save(conversation);
}

const DEFAULT_TITLE = 'New build';

// A chat is empty until its first message is stored: a new-chat view makes
// one before anything is sent. An empty chat is not listed and not found by
// search, and one nothing uses is removed (clearExpiredArchive).
const emptyChat = (chat) => `NOT EXISTS (SELECT 1 FROM messages WHERE messages.conversation_id = ${chat}.id)`;

function newConversation(provider, model, { title = DEFAULT_TITLE, effort = null, resolution = 1, benchId = null, benchSlot = null } = {}) {
  if (!provider || !model) throw new Error('Conversation provider and model are required');
  return {
    id: crypto.randomUUID(),
    title,
    provider,
    model,
    effort,
    resolution,
    benchId,
    benchSlot,
    createdAt: new Date().toISOString(),
    messages: [],
    currentMessageId: null,
    nextMessagePosition: 0,
    world: new World(),
    checkpoints: new Map(),
    unsaved: new Set(),
  };
}

function createConversation(provider, model, options) {
  const conversation = newConversation(provider, model, options);
  live.set(conversation.id, conversation);
  save(conversation);
  return conversation;
}

function conversationMessageRows(id) {
  return database.prepare(`
    SELECT * FROM messages WHERE conversation_id = ? ORDER BY position
  `).all(id);
}

function activeMessageRows(id, messageRows, headId) {
  const byId = new Map(messageRows.map((message) => [message.id, message]));
  const activeRows = [];
  const seen = new Set();
  let currentId = headId;
  while (currentId) {
    if (seen.has(currentId)) throw new Error(`Conversation ${id} contains a message cycle`);
    seen.add(currentId);
    const message = byId.get(currentId);
    if (!message) throw new Error(`Conversation ${id} references missing message ${currentId}`);
    activeRows.push(message);
    currentId = message.parent_id;
  }
  activeRows.reverse();
  return activeRows;
}

function loadBranch(id, messageRows, headId, worldManifestJson) {
  const activeRows = activeMessageRows(id, messageRows, headId);
  const readChunk = chunkReader(id);
  const chunkCache = new Map();
  const world = World.fromManifest(parseJson(worldManifestJson, `Conversation ${id} world`), readChunk, chunkCache);
  const checkpoints = new Map();
  const messages = activeRows.map((message) => {
    const payload = parseJson(message.payload_json, `Conversation ${id} message ${message.id}`);
    if (message.checkpoint_manifest_json) {
      const manifest = parseJson(message.checkpoint_manifest_json, `Conversation ${id} checkpoint ${message.id}`);
      checkpoints.set(message.id, World.fromManifest(manifest, readChunk, chunkCache));
    }
    return { ...payload, id: message.id, parentId: message.parent_id, position: message.position };
  });
  return {
    messages,
    currentMessageId: headId || null,
    nextMessagePosition: messageRows.length ? messageRows[messageRows.length - 1].position + 1 : 0,
    world,
    checkpoints,
    unsaved: new Set(),
  };
}

function getConversation(id) {
  if (live.has(id)) return live.get(id);
  const row = database.prepare(`
    SELECT * FROM conversations WHERE id = ? AND archived_at IS NULL
  `).get(id);
  if (!row) return null;

  const messageRows = conversationMessageRows(id);
  const branch = loadBranch(id, messageRows, row.current_message_id, row.world_manifest_json);
  // Measured now, so its first write finds the measurement in.
  storage.measure(id);
  const conversation = {
    id: row.id,
    title: row.title,
    provider: row.provider,
    model: row.model,
    effort: row.effort || null,
    resolution: row.resolution,
    benchId: row.bench_id,
    benchSlot: row.bench_slot,
    createdAt: row.created_at,
    ...branch,
  };
  live.set(id, conversation);
  return conversation;
}

function messageTree(conversation) {
  const messages = database.prepare(`
    SELECT id, parent_id, position,
      json_extract(payload_json, '$.role') AS role,
      json_extract(payload_json, '$.seeBuild') AS see_build
    FROM messages WHERE conversation_id = ? ORDER BY position
  `).all(conversation.id).map((row) => ({
    id: row.id,
    parentId: row.parent_id,
    position: row.position,
    role: row.role,
    branchable: row.role === 'user' && !row.see_build,
  }));
  return { headId: conversation.currentMessageId || null, messages };
}

function activateBranch(conversation, messageId) {
  const messageRows = conversationMessageRows(conversation.id);
  const byId = new Map(messageRows.map((message) => [message.id, message]));
  let head = byId.get(messageId);
  if (!head) throw refused('Unknown message branch');

  const children = new Map();
  for (const message of messageRows) {
    const siblings = children.get(message.parent_id) || [];
    siblings.push(message);
    children.set(message.parent_id, siblings);
  }
  const seen = new Set();
  while (children.has(head.id)) {
    if (seen.has(head.id)) throw new Error(`Conversation ${conversation.id} contains a message cycle`);
    seen.add(head.id);
    head = children.get(head.id).at(-1);
  }
  moveHead(conversation, messageRows, head);
}

// Puts a bench chat on the branch that answered `roundIds` in order, as far
// as it got: its head becomes the end of its turn on the last of those rounds
// it ran. Within a turn there are no branches; a later prompt hangs off the
// message that ended the turn before it.
function followRounds(conversation, roundIds) {
  const messageRows = conversationMessageRows(conversation.id);
  const payloads = new Map(messageRows.map((row) => [row.id, JSON.parse(row.payload_json)]));
  const isPrompt = (row) => payloads.get(row.id).role === 'user' && !payloads.get(row.id).seeBuild;
  const children = new Map();
  for (const row of messageRows) {
    const siblings = children.get(row.parent_id) || [];
    siblings.push(row);
    children.set(row.parent_id, siblings);
  }
  let head = null;
  for (const roundId of roundIds) {
    const prompt = (children.get(head ? head.id : null) || []).filter((row) => isPrompt(row) && payloads.get(row.id).round === roundId).at(-1);
    if (!prompt) break;
    for (let next = prompt; next;) {
      head = next;
      next = (children.get(next.id) || []).filter((row) => !isPrompt(row)).at(-1);
    }
  }
  if (head) return moveHead(conversation, messageRows, head);
  const first = conversation.messages.find((message) => message.role === 'user' && !message.seeBuild);
  if (first) revert(conversation, first.id);
}

function moveHead(conversation, messageRows, head) {
  if (!head.world_manifest_json) throw refused('Selected branch has no saved world state');

  Object.assign(conversation, loadBranch(conversation.id, messageRows, head.id, head.world_manifest_json));
  save(conversation);
}

const upsertConversation = database.prepare(`
  INSERT INTO conversations (
    id, title, provider, model, effort, resolution, created_at,
    current_message_id, world_manifest_json, archived_at, bench_id, bench_slot
  ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
  ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    provider = excluded.provider,
    model = excluded.model,
    effort = excluded.effort,
    resolution = excluded.resolution,
    current_message_id = excluded.current_message_id,
    world_manifest_json = excluded.world_manifest_json
  WHERE conversations.archived_at IS NULL
`);
const upsertMessage = database.prepare(`
  INSERT INTO messages (
    id, conversation_id, parent_id, position, payload_json, checkpoint_manifest_json
  ) VALUES (?, ?, ?, ?, ?, ?)
  ON CONFLICT(id) DO UPDATE SET
    payload_json = excluded.payload_json,
    checkpoint_manifest_json = excluded.checkpoint_manifest_json
  WHERE messages.conversation_id = excluded.conversation_id
`);
// Every message that was ever the head keeps the world as of that point, so
// activating an older branch can restore it.
const setHeadWorld = database.prepare(`
  UPDATE messages SET world_manifest_json = ? WHERE id = ? AND conversation_id = ?
`);

// Writes the conversation row, the current head's world, and only the
// messages appended or changed since the last save. Message rows are
// immutable once written except for the error-dismissed flag. Memory is
// never ahead of the database: a save that fails drops the conversation
// from `live`, so the next getConversation loads what is stored, and the
// dropped object can never be saved again.
function save(conversation) {
  if (live.get(conversation.id) !== conversation) throw new Error(`Conversation ${conversation.id} is no longer loaded (archived, or dropped by a failed save)`);
  const written = new Map();
  try {
    const writeChunk = chunkWriter(conversation.id, written);
    const worldJson = JSON.stringify(conversation.world.serializeManifest(writeChunk));
    const head = conversation.messages.at(-1)?.id ?? null;
    if ((conversation.currentMessageId || null) !== head) {
      throw new Error(`Conversation ${conversation.id} has an invalid active head`);
    }
    const rows = conversation.messages
      .filter((message) => conversation.unsaved.has(message.id))
      .map((message) => ({
        id: message.id,
        parentId: message.parentId || null,
        position: message.position,
        payload: JSON.stringify(messagePayload(message)),
        checkpoint: conversation.checkpoints.has(message.id)
          ? JSON.stringify(conversation.checkpoints.get(message.id).serializeManifest(writeChunk))
          : null,
      }));

    database.transaction(() => {
      const result = upsertConversation.run(
        conversation.id,
        conversation.title,
        conversation.provider,
        conversation.model,
        conversation.effort || null,
        conversation.resolution,
        conversation.createdAt,
        conversation.currentMessageId || null,
        worldJson,
        conversation.benchId,
        conversation.benchSlot,
      );
      if (result.changes !== 1) throw new Error(`Conversation ${conversation.id} is archived`);
      for (const row of rows) {
        const written = upsertMessage.run(row.id, conversation.id, row.parentId, row.position, row.payload, row.checkpoint);
        if (written.changes !== 1) throw new Error(`Message ${row.id} belongs to another conversation`);
        const text = messageText(conversation.messages.find((message) => message.id === row.id));
        if (text.trim()) indexDoc.run(conversation.id, `message:${row.id}`, text);
      }
      indexDoc.run(conversation.id, `title:${conversation.id}`, conversation.title);
      if (head) setHeadWorld.run(worldJson, head, conversation.id);
    })();
    conversation.unsaved.clear();
  } catch (error) {
    discardChunks(conversation.id, written, [conversation.world, ...conversation.checkpoints.values()]);
    live.delete(conversation.id);
    throw error;
  }
}

function revert(conversation, messageId) {
  const index = conversation.messages.findIndex((message) => message.id === messageId);
  const target = conversation.messages[index];
  if (!target || target.role !== 'user' || target.seeBuild) {
    throw refused('messageId must point at a user message');
  }
  const checkpoint = conversation.checkpoints.get(messageId);
  if (!checkpoint) throw refused('No world checkpoint for this message');

  conversation.messages = conversation.messages.slice(0, index);
  conversation.currentMessageId = target.parentId || null;
  conversation.world = checkpoint.reference();
  const kept = new Set(conversation.messages.map((message) => message.id));
  conversation.checkpoints = new Map([...conversation.checkpoints].filter(([id]) => kept.has(id)));
  conversation.unsaved = new Set([...conversation.unsaved].filter((id) => kept.has(id)));
  save(conversation);
  return target;
}

function archiveConversation(id) {
  const result = database.prepare(`
    UPDATE conversations SET archived_at = ? WHERE id = ? AND archived_at IS NULL
  `).run(Date.now(), id);
  if (result.changes !== 1) throw new Error(`Conversation ${id} not found`);
  live.delete(id);
}

// Deletes chats `ids` for good: their rows and search entries in one
// transaction, then their folders, harness processes and sessions, and
// storage measurements. Rows go first, so a file that cannot be removed
// leaves an orphan file, never a chat missing its files. The 7-day purge and
// a permanent delete are this.
function removeChats(ids, extra = () => {}) {
  database.transaction(() => {
    for (const id of ids) {
      database.prepare('DELETE FROM search_docs WHERE owner = ?').run(id);
      database.prepare('DELETE FROM conversations WHERE id = ?').run(id);
    }
    extra();
  })();
  for (const id of ids) {
    live.delete(id);
    removeConversationArtifacts(id);
    harness.destroy(id);
    storage.forget(id);
  }
}

// A bench, its votes and its models' chats, for good.
function removeBench(id) {
  const members = database.prepare('SELECT id FROM conversations WHERE bench_id = ?').pluck().all(id);
  removeChats(members, () => {
    database.prepare('DELETE FROM search_docs WHERE owner = ?').run(id);
    database.prepare('DELETE FROM bench_votes WHERE bench_id = ?').run(id);
    database.prepare('DELETE FROM benches WHERE id = ?').run(id);
  });
  fs.rmSync(path.join(attachmentsDir, id), { recursive: true, force: true });
  storage.forget(id);
}

// What is archived, newest first: Build chats (a bench's chats are part of
// it) and benches with how many models they ran. Each is deleted at
// `expiresAt`, epoch ms like `archivedAt`.
function listArchive() {
  const expiry = (row) => ({ ...row, expiresAt: row.archivedAt + ARCHIVE_TTL_MS });
  return {
    conversations: database.prepare(`
      SELECT id, title, provider, model, archived_at AS archivedAt FROM conversations
      WHERE archived_at IS NOT NULL AND bench_id IS NULL ORDER BY archived_at DESC
    `).all().map(expiry),
    benches: database.prepare(`
      SELECT id, title, archived_at AS archivedAt, (SELECT count(*) FROM conversations WHERE bench_id = benches.id) AS models
      FROM benches WHERE archived_at IS NOT NULL ORDER BY archived_at DESC
    `).all().map(({ models, ...row }) => ({ ...expiry(row), models })),
  };
}

// Throws unless `id` is an archived Build chat; `unarchived` is what a chat
// that is not archived gets.
function checkArchivedChat(id, unarchived) {
  const row = database.prepare('SELECT bench_id, archived_at FROM conversations WHERE id = ?').get(id);
  if (!row) throw refused('Conversation not found', 404);
  if (row.bench_id) throw refused('A bench chat is changed only through its bench', 409);
  if (row.archived_at === null) throw unarchived;
}

function checkArchivedBench(id, unarchived) {
  const row = database.prepare('SELECT archived_at FROM benches WHERE id = ?').get(id);
  if (!row) throw refused('Bench not found', 404);
  if (row.archived_at === null) throw unarchived;
}

function restoreConversation(id) {
  checkArchivedChat(id, refused('This chat is not archived', 404));
  database.prepare('UPDATE conversations SET archived_at = NULL WHERE id = ?').run(id);
}

// With its models' chats, which were archived with it.
function restoreBench(id) {
  checkArchivedBench(id, refused('This bench is not archived', 404));
  database.transaction(() => {
    database.prepare('UPDATE benches SET archived_at = NULL WHERE id = ?').run(id);
    database.prepare('UPDATE conversations SET archived_at = NULL WHERE bench_id = ?').run(id);
  })();
}

function deleteConversation(id) {
  checkArchivedChat(id, refused('Archive it first', 409));
  removeChats([id]);
}

function deleteBench(id) {
  checkArchivedBench(id, refused('Archive it first', 409));
  removeBench(id);
}

function clearExpiredArchive(now = Date.now()) {
  const cutoff = now - ARCHIVE_TTL_MS;
  const ids = database.prepare(`
    SELECT id FROM conversations WHERE archived_at IS NOT NULL AND archived_at <= ? AND bench_id IS NULL
  `).pluck().all(cutoff);
  // An empty chat goes after the same wait, unless this run of the server
  // made it or was asked for it (it is in `live`): an open page may hold it
  // and send its first message yet. Its files are those of a refused message.
  const empty = database.prepare(`
    SELECT id FROM conversations WHERE archived_at IS NULL AND bench_id IS NULL AND created_at <= ? AND ${emptyChat('conversations')}
  `).pluck().all(new Date(cutoff).toISOString()).filter((id) => !live.has(id));
  for (const id of [...ids, ...empty]) {
    try {
      removeChats([id]);
    } catch (error) {
      console.warn(`[store] archive cleanup failed for ${id}: ${error.message}`);
    }
  }
  const benches = database.prepare('SELECT id FROM benches WHERE archived_at IS NOT NULL AND archived_at <= ?').pluck().all(cutoff);
  for (const id of benches) {
    try {
      removeBench(id);
    } catch (error) {
      console.warn(`[store] archive cleanup failed for bench ${id}: ${error.message}`);
    }
  }
}

// A first prompt resent unchanged (same text and attachment bytes) keeps the
// title generated for the latest root prompt instead of paying for a new one.
function repeatsLatestRootPrompt(conversation, content) {
  const row = database.prepare(`
    SELECT payload_json FROM messages
    WHERE conversation_id = ? AND parent_id IS NULL AND json_extract(payload_json, '$.role') = 'user'
    ORDER BY position DESC LIMIT 1
  `).get(conversation.id);
  if (!row) return false;
  const signature = (value) => JSON.stringify((typeof value === 'string' ? [{ type: 'text', text: value }] : value).map((part) => (
    part.type === 'attachment'
      ? { name: part.attachment.name, type: part.attachment.type, sha256: crypto.createHash('sha256').update(readAttachment(part.attachment.url)).digest('hex') }
      : part
  )));
  return signature(parseJson(row.payload_json, 'message payload').content) === signature(content);
}

function setTitle(conversation, title) {
  // Titles arrive while a turn is running. Persist just the metadata so list
  // refreshes see it without checkpointing the turn's unfinished work.
  const result = database.prepare(`
    UPDATE conversations SET title = ? WHERE id = ? AND archived_at IS NULL
  `).run(title, conversation.id);
  if (result.changes !== 1) throw new Error(`Conversation ${conversation.id} not found or archived`);
  indexDoc.run(conversation.id, `title:${conversation.id}`, title);
  conversation.title = title;
}

// A turn finished after the user last looked at the chat.
function markFinished(id) {
  database.prepare('UPDATE conversations SET finished_at = ? WHERE id = ?').run(Date.now(), id);
}

function markSeen(id) {
  database.prepare('UPDATE conversations SET seen_at = ? WHERE id = ?').run(Date.now(), id);
}

// Every chat stored, archived ones and bench models' chats included, with
// whether it is archived.
function listConversationIds() {
  return database.prepare('SELECT id, archived_at IS NOT NULL AS archived FROM conversations').all()
    .map((row) => ({ id: row.id, archived: Boolean(row.archived) }));
}

// Every bench stored, archived ones included, with whether it is archived.
function listBenchIds() {
  return database.prepare('SELECT id, archived_at IS NOT NULL AS archived FROM benches').all()
    .map((row) => ({ id: row.id, archived: Boolean(row.archived) }));
}

function listConversations() {
  return database.prepare(`
    SELECT id, title, provider, model, effort, resolution, created_at AS createdAt,
      bench_id AS benchId, bench_slot AS benchSlot,
      COALESCE(finished_at > COALESCE(seen_at, 0), 0) AS unread,
      (SELECT count(*) FROM messages WHERE messages.conversation_id = conversations.id) AS messageCount
    FROM conversations
    WHERE archived_at IS NULL AND NOT (${emptyChat('conversations')})
    ORDER BY created_at DESC
  `).all().map((row) => ({ ...row, effort: row.effort || null, unread: Boolean(row.unread) }));
}

// A bench is one prompt and its follow-ups (rounds), each sent unchanged to
// every model in it. Each model's run is an ordinary chat tagged with the
// bench. A bench is written whole, with its first round (`content`, whose
// files are already stored under `id`) and its models' chats, or not at all.
// Blind letters run in a random order, so the order models were added in
// tells nothing.
function createBench(id, { title, blind, resolution, content, members }) {
  const chats = members.map(({ provider, model, effort }, slot) => newConversation(provider, model, { title, effort, resolution, benchId: id, benchSlot: slot }));
  const order = chats.map((chat) => chat.id);
  if (blind) {
    for (let i = order.length - 1; i > 0; i -= 1) {
      const j = crypto.randomInt(i + 1);
      [order[i], order[j]] = [order[j], order[i]];
    }
  }
  const round = { id: crypto.randomUUID(), parentId: null, content, createdAt: new Date().toISOString() };
  try {
    database.transaction(() => {
      database.prepare(`
        INSERT INTO benches (id, title, created_at, rounds_json, head_round_id, blind, order_json) VALUES (?, ?, ?, ?, ?, ?, ?)
      `).run(id, title, round.createdAt, JSON.stringify([round]), round.id, blind ? 1 : 0, JSON.stringify(order));
      indexDoc.run(id, `title:${id}`, title);
      indexDoc.run(id, `round:${id}:${round.id}`, messageText({ role: 'user', content }));
      for (const chat of chats) {
        live.set(chat.id, chat);
        save(chat);
      }
    })();
  } catch (error) {
    for (const chat of chats) live.delete(chat.id);
    throw error;
  }
  return { bench: getBench(id), members: chats };
}

// The rounds from the first to `headId`, each with where it stands among the
// rounds sent in its place: { index, count, previousId, nextId }.
function roundChain(nodes, headId) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const rounds = [];
  for (let node = byId.get(headId); node; node = byId.get(node.parentId)) {
    const siblings = nodes.filter((other) => other.parentId === node.parentId);
    const index = siblings.indexOf(node);
    rounds.push({
      ...node,
      branch: { index, count: siblings.length, previousId: siblings[index - 1]?.id || null, nextId: siblings[index + 1]?.id || null },
    });
  }
  return rounds.reverse();
}

// A round's files are stored under the bench; each chat gets its own copy,
// which counts against that chat's budget.
function shareAttachment(attachment, conversationId) {
  const name = path.basename(attachment.url);
  const target = path.join(attachmentsDir, conversationId, name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  if (!fs.existsSync(target)) {
    storage.reserve(conversationId, attachment.size);
    fs.copyFileSync(attachmentFile(attachment.url), target, fs.constants.COPYFILE_FICLONE);
  }
  return { ...attachment, url: `/api/attachments/${conversationId}/${name}` };
}

// How a stored head ends its turn: 'final' (the model's last answer),
// 'stopped', 'failed' (an error), or null for a turn still under way (a
// prompt, a call or its results, screenshots).
function turnEnd(message) {
  if (message.role === 'error') return 'failed';
  if (message.role !== 'assistant' || message.tool_calls?.length) return null;
  return message.stopped ? 'stopped' : 'final';
}

// How far a bench chat's stored branch has got: the rounds it was sent, and
// whether its last turn failed or was stopped, which holds its queue. A turn
// still under way reads as failed: only the caller knows if one is running.
function benchProgress(id) {
  const row = database.prepare(`
    WITH RECURSIVE chain(id, parent_id, payload_json, depth) AS (
      SELECT id, parent_id, payload_json, 0 FROM messages
        WHERE id = (SELECT current_message_id FROM conversations WHERE id = ?)
      UNION ALL
      SELECT m.id, m.parent_id, m.payload_json, chain.depth + 1 FROM messages m JOIN chain ON m.id = chain.parent_id
    )
    SELECT
      COALESCE(SUM(json_extract(payload_json, '$.role') = 'user' AND COALESCE(json_extract(payload_json, '$.seeBuild'), 0) = 0), 0) AS round,
      MAX(CASE WHEN depth = 0 THEN payload_json END) AS head
    FROM chain
  `).get(id);
  const end = row.head ? turnEnd(JSON.parse(row.head)) : 'final';
  return { round: row.round, halted: end === 'final' ? null : end === 'stopped' ? 'stopped' : 'failed' };
}

// The result of a tool call its turn ended without answering: stopped by
// the user, or cut off by `reason`.
function unansweredResult(stopped, reason) {
  return stopped
    ? { ok: false, cancelled: true, error: 'Cancelled by user' }
    : { ok: false, interrupted: true, error: `This tool call did not finish (${reason}). Its side effects are unknown; inspect the build or workspace before retrying a change.` };
}

// Ends the turn at the head of `conversation` with `final` (an error, or an
// assistant message flagged `stopped`), and saves it. Each tool call the turn
// left without a result gets one first, so every stored call has its result.
function endTurn(conversation, final) {
  let at = conversation.messages.length - 1;
  while (conversation.messages[at].role === 'tool' || conversation.messages[at].seeBuild) at -= 1;
  const answered = new Set(conversation.messages.slice(at + 1).map((message) => message.tool_call_id));
  const unanswered = unansweredResult(final.stopped, final.content);
  for (const call of conversation.messages[at].tool_calls ?? []) {
    if (!answered.has(call.id)) appendMessage(conversation, { role: 'tool', tool_call_id: call.id, content: JSON.stringify(unanswered) });
  }
  const message = appendMessage(conversation, final);
  save(conversation);
  return message;
}

// Ends the turn at the head of `conversation` as failed, with `reason`, if
// that turn is still under way. Only for a chat nothing holds: a running
// turn's head is under way too. Returns whether it ended one.
function endUnfinishedTurn(conversation, reason) {
  const head = conversation.messages.at(-1);
  if (!head || turnEnd(head)) return false;
  endTurn(conversation, { role: 'error', content: reason });
  return true;
}

// Ends every stored turn the server stopped in the middle of (a crash, a
// kill), so every stored head ends a turn. Returns the chats it ended. A chat
// that cannot be read or loaded (bad JSON, a missing chunk file) is logged
// and left as stored, so it does not stop the others or the server's start.
const INTERRUPTED = 'The server stopped during this turn';
// The same for a turn of this run: saving how it ended failed.
const UNSAVED = 'How this turn ended was not saved';
function interruptUnfinished() {
  const heads = database.prepare(`
    SELECT c.id, m.payload_json FROM conversations c JOIN messages m ON m.id = c.current_message_id
    WHERE c.archived_at IS NULL
  `).all();
  return heads.flatMap(({ id, payload_json: payload }) => {
    try {
      if (turnEnd(parseJson(payload, `Conversation ${id} head`))) return [];
      endUnfinishedTurn(getConversation(id), INTERRUPTED);
      return [id];
    } catch (error) {
      console.error(`[store] the turn the server stopped in ${id} could not be ended:`, error);
      return [];
    }
  });
}

// `nodes` holds every round ever sent; `rounds` the active line of them.
function getBench(id) {
  const row = database.prepare('SELECT * FROM benches WHERE id = ? AND archived_at IS NULL').get(id);
  if (!row) return null;
  const nodes = parseJson(row.rounds_json, `Bench ${id} rounds`);
  const votes = Object.fromEntries(database.prepare('SELECT round_id, pick, blind FROM bench_votes WHERE bench_id = ?').all(id)
    .map((vote) => [vote.round_id, { pick: vote.pick, blind: Boolean(vote.blind) }]));
  return {
    id: row.id,
    title: row.title,
    createdAt: row.created_at,
    head: row.head_round_id,
    nodes,
    rounds: roundChain(nodes, row.head_round_id),
    blind: Boolean(row.blind),
    revealed: row.revealed_at !== null,
    order: JSON.parse(row.order_json),
    votes,
  };
}

function revealBench(bench) {
  database.prepare('UPDATE benches SET revealed_at = ? WHERE id = ? AND revealed_at IS NULL').run(Date.now(), bench.id);
  bench.revealed = true;
}

// `pick` is a member id, or null for a tie, among the `judged` member ids.
// A vote is cast while the bench hides its models' names, and counts toward
// the leaderboard. (`blind` is 0 on votes kept from when a bench that showed
// its names took them; those never counted.)
function castVote(bench, roundId, pick, judged) {
  if (!bench.blind || bench.revealed) throw new Error(`Bench ${bench.id} shows its models' names: it takes no votes`);
  database.prepare(`
    INSERT INTO bench_votes (round_id, bench_id, pick, members_json, blind, created_at) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(round_id) DO UPDATE SET pick = excluded.pick, members_json = excluded.members_json, blind = excluded.blind, created_at = excluded.created_at
  `).run(roundId, bench.id, pick, JSON.stringify(judged), 1, Date.now());
  bench.votes[roundId] = { pick, blind: true };
}

function voteCount() {
  return database.prepare('SELECT COUNT(*) AS count FROM bench_votes').get().count;
}

function clearAllVotes() {
  database.prepare('DELETE FROM bench_votes').run();
}

function clearVote(bench, roundId) {
  database.prepare('DELETE FROM bench_votes WHERE round_id = ?').run(roundId);
  delete bench.votes[roundId];
}

// Ratings from every blind vote in benches still kept, oldest first. A pick
// beats each other model judged with it; a tie draws them all. A model at
// two efforts is two entries.
function leaderboard() {
  const members = new Map(database.prepare('SELECT id, provider, model, effort FROM conversations WHERE bench_id IS NOT NULL').all().map((row) => [row.id, row]));
  const entries = new Map();
  const entry = (id) => {
    const { provider, model, effort } = members.get(id);
    const key = `${model}\u0000${effort || ''}`;
    if (!entries.has(key)) entries.set(key, { provider, model, effort: effort || null, rating: 1000, wins: 0, votes: 0, benches: new Set() });
    return entries.get(key);
  };
  const votes = database.prepare(`
    SELECT v.bench_id, v.pick, v.members_json FROM bench_votes v JOIN benches b ON b.id = v.bench_id
    WHERE v.blind = 1 AND b.archived_at IS NULL ORDER BY v.created_at
  `).all();
  for (const vote of votes) {
    const judged = JSON.parse(vote.members_json).map((id) => ({ id, entry: entry(id) }));
    const before = new Map(judged.map(({ entry: e }) => [e, e.rating]));
    const play = (a, b, score) => {
      const expected = 1 / (1 + 10 ** ((before.get(b) - before.get(a)) / 400));
      a.rating += 24 * (score - expected);
      b.rating -= 24 * (score - expected);
    };
    for (const { id, entry: e } of judged) {
      e.votes += 1;
      e.benches.add(vote.bench_id);
      if (id === vote.pick) e.wins += 1;
    }
    for (let i = 0; i < judged.length; i += 1) {
      for (let j = i + 1; j < judged.length; j += 1) {
        const [a, b] = [judged[i], judged[j]];
        if (vote.pick === null) play(a.entry, b.entry, 0.5);
        else if (a.id === vote.pick) play(a.entry, b.entry, 1);
        else if (b.id === vote.pick) play(b.entry, a.entry, 1);
      }
    }
  }
  return [...entries.values()]
    .map(({ benches, rating, ...rest }) => ({ ...rest, rating: Math.round(rating), benches: benches.size }))
    .sort((a, b) => b.rating - a.rating);
}

function appendRound(bench, content) {
  const node = { id: crypto.randomUUID(), parentId: bench.head, content, createdAt: new Date().toISOString() };
  database.transaction(() => {
    database.prepare('UPDATE benches SET rounds_json = ?, head_round_id = ? WHERE id = ?').run(JSON.stringify([...bench.nodes, node]), node.id, bench.id);
    indexDoc.run(bench.id, `round:${bench.id}:${node.id}`, messageText({ role: 'user', content }));
  })();
  Object.assign(bench, getBench(bench.id));
}

// Makes the line ending at `headId` (null: no rounds) the bench's active one.
function setBenchHead(bench, headId) {
  database.prepare('UPDATE benches SET head_round_id = ? WHERE id = ?').run(headId, bench.id);
  Object.assign(bench, getBench(bench.id));
}

const prompts = (conversation) => conversation.messages.filter((message) => message.role === 'user' && !message.seeBuild);

// A change to a bench's line and to its models' chats is stored whole or not
// at all: the models stay on the rounds the line names. One that fails drops
// the chats it may have changed from `live`, as a failed save does, and
// `bench` is read again, so memory is what is stored.
function benchChange(bench, members, change) {
  try {
    database.transaction(change)();
  } catch (error) {
    for (const member of members) live.delete(member.id);
    Object.assign(bench, getBench(bench.id));
    throw error;
  }
}

// Goes back to before round `index` of the line: every model sent it returns
// to its world then, and the line ends at the round before. Returns those
// models.
function revertBenchRound(bench, index) {
  const reverted = benchMembers(bench.id).filter((member) => prompts(member)[index]);
  benchChange(bench, reverted, () => {
    for (const member of reverted) revert(member, prompts(member)[index].id);
    setBenchHead(bench, bench.rounds[index].parentId);
  });
  return reverted;
}

// Makes the latest line through round `roundId` the active one, and puts
// each model's chat on the branch that answered it.
function followBenchBranch(bench, roundId) {
  const members = benchMembers(bench.id);
  benchChange(bench, members, () => {
    let head = roundId;
    for (let child; (child = bench.nodes.filter((node) => node.parentId === head).at(-1));) head = child.id;
    setBenchHead(bench, head);
    const line = bench.rounds.map((round) => round.id);
    for (const member of members) followRounds(member, line);
  });
}

// A model's last round is run again: its chat goes back to before it, and
// the vote on that round, whose builds are changing, is taken back.
function retryBenchRound(bench, member) {
  const sent = prompts(member);
  benchChange(bench, [member], () => {
    clearVote(bench, bench.rounds[sent.length - 1].id);
    revert(member, sent.at(-1).id);
  });
}

function setBenchTitle(bench, title) {
  database.transaction(() => {
    database.prepare('UPDATE benches SET title = ? WHERE id = ?').run(title, bench.id);
    database.prepare('UPDATE conversations SET title = ? WHERE bench_id = ?').run(title, bench.id);
    indexDoc.run(bench.id, `title:${bench.id}`, title);
  })();
  bench.title = title;
  for (const member of benchMembers(bench.id)) member.title = title;
}

// Sidebar search, best match first. Build finds chats; Bench finds benches by
// their title, rounds and anything their models said. Each result carries a
// snippet of the first matching message or round, the match wrapped in
// \u0001 and \u0002.
// The query's time grows with the square of its terms and SQLite blocks the
// server while it runs, so a search takes at most MAX_SEARCH_TERMS words.
const MAX_SEARCH_TERMS = 16;
function search(scope, query) {
  const terms = [...new Set(query.match(/[\p{L}\p{N}_]+/gu) || [])];
  if (!terms.length) return [];
  if (terms.length > MAX_SEARCH_TERMS) {
    throw refused(`A search takes at most ${MAX_SEARCH_TERMS} different words`);
  }
  const rows = database.prepare(`
    SELECT d.owner, d.source, snippet(search, 0, char(1), char(2), '…', 12) AS snippet,
      c.id AS chatId, c.bench_id AS benchId, c.archived_at AS chatArchived, ${emptyChat('c')} AS chatEmpty,
      b.id AS bench, b.archived_at AS benchArchived
    FROM search
    JOIN search_docs d ON d.id = search.rowid
    LEFT JOIN conversations c ON c.id = d.owner
    LEFT JOIN benches b ON b.id = COALESCE(c.bench_id, d.owner)
    WHERE search MATCH ?
    ORDER BY bm25(search)
    LIMIT 500
  `).all(terms.map((term) => `"${term}"*`).join(' '));
  const results = new Map();
  for (const row of rows) {
    const id = scope === 'build'
      ? (row.chatId && !row.benchId && row.chatArchived === null && !row.chatEmpty ? row.chatId : null)
      : (row.bench && row.benchArchived === null && (!row.chatId || row.chatArchived === null) ? row.bench : null);
    if (!id) continue;
    if (!results.has(id)) results.set(id, { id, snippet: null });
    const result = results.get(id);
    if (!result.snippet && !row.source.startsWith('title:')) result.snippet = row.snippet;
  }
  return [...results.values()];
}

function listBenches() {
  return database.prepare(`
    SELECT id, title, created_at AS createdAt, rounds_json, head_round_id AS head, blind, revealed_at, order_json,
      -- A bench's resolution is its models' chats': all are made with the bench's.
      (SELECT max(resolution) FROM conversations WHERE conversations.bench_id = benches.id) AS resolution
    FROM benches WHERE archived_at IS NULL ORDER BY created_at DESC
  `).all().map(({ rounds_json: nodes, blind, revealed_at: revealedAt, order_json: order, ...row }) => ({
    ...row,
    rounds: roundChain(JSON.parse(nodes), row.head).length,
    blind: Boolean(blind),
    revealed: revealedAt !== null,
    order: JSON.parse(order),
  }));
}

function benchMembers(benchId) {
  return database.prepare(`
    SELECT id FROM conversations WHERE bench_id = ? AND archived_at IS NULL ORDER BY bench_slot
  `).all(benchId).map((row) => getConversation(row.id));
}

function archiveBench(id) {
  const members = database.prepare('SELECT id FROM conversations WHERE bench_id = ? AND archived_at IS NULL').all(id);
  database.transaction(() => {
    const result = database.prepare('UPDATE benches SET archived_at = ? WHERE id = ? AND archived_at IS NULL').run(Date.now(), id);
    if (result.changes !== 1) throw new Error(`Bench ${id} not found`);
    for (const { id: memberId } of members) archiveConversation(memberId);
  })();
  return members.map((row) => row.id);
}

// A new chat `id` continuing `source` from `messageCount` messages in, with
// `world`. Files the copied messages reference move with it, so it outlives
// the source. Its files are copied before its rows are saved: a fork that
// fails at either leaves neither.
function forkConversation(source, messageCount, world, id = crypto.randomUUID()) {
  try {
    for (const dir of [worldsDir, imagesDir, attachmentsDir]) {
      const from = path.join(dir, source.id);
      if (fs.existsSync(from)) fs.cpSync(from, path.join(dir, id), { recursive: true, mode: fs.constants.COPYFILE_FICLONE });
    }
    return saveFork(source, messageCount, world, id);
  } catch (error) {
    removeConversationArtifacts(id);
    storage.forget(id);
    throw error;
  }
}

function saveFork(source, messageCount, world, id) {
  const rewrite = (value) => JSON.parse(JSON.stringify(value)
    .replaceAll(`/api/images/${source.id}/`, `/api/images/${id}/`)
    .replaceAll(`/api/attachments/${source.id}/`, `/api/attachments/${id}/`));
  const ids = new Map();
  const messages = [];
  for (const message of source.messages.slice(0, messageCount)) {
    const copy = { ...rewrite(messagePayload(message)), id: crypto.randomUUID(), parentId: messages.at(-1)?.id ?? null, position: messages.length };
    ids.set(message.id, copy.id);
    messages.push(copy);
  }
  const conversation = {
    id,
    title: source.title,
    provider: source.provider,
    model: source.model,
    effort: source.effort,
    resolution: source.resolution,
    benchId: null,
    benchSlot: null,
    createdAt: new Date().toISOString(),
    messages,
    currentMessageId: messages.at(-1)?.id ?? null,
    nextMessagePosition: messages.length,
    world: world.reference(),
    checkpoints: new Map([...source.checkpoints].filter(([key]) => ids.has(key)).map(([key, value]) => [ids.get(key), value])),
    unsaved: new Set(messages.map((message) => message.id)),
  };
  live.set(id, conversation);
  save(conversation);
  return conversation;
}

function close() {
  live.clear();
  database.close();
}

module.exports = {
  createConversation, getConversation, appendMessage, dismissError, messagePayload, publicMessages, save, revert,
  messageTree, activateBranch, setTitle, repeatsLatestRootPrompt,
  archiveConversation, clearExpiredArchive, listArchive, restoreConversation, restoreBench, deleteConversation, deleteBench, listConversations, listConversationIds, listBenchIds, markFinished, markSeen, close,
  createBench, getBench, appendRound, revertBenchRound, followBenchBranch, retryBenchRound, revealBench, castVote, clearVote, voteCount, clearAllVotes, leaderboard, setBenchTitle, listBenches, benchMembers, archiveBench, forkConversation,
  shareAttachment, benchProgress, endTurn, unansweredResult, endUnfinishedTurn, interruptUnfinished, INTERRUPTED, UNSAVED, search, collectChunks, persistWorld, reportReferences,
  saveImages, readImage, imageFile, saveAttachment, saveAttachments, discardAttachments, readAttachment, attachmentFile,
  IMAGES_DIR: imagesDir,
  ATTACHMENTS_DIR: attachmentsDir,
  DEFAULT_TITLE,
  DATABASE_FILE: databaseFile,
  ARCHIVE_TTL_MS,
};
