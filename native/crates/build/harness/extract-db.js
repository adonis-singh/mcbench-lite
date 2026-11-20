'use strict';

// node extract-db.js <db copy> <worlds copy> <sandbox copy> <blocks.json> <corpus>
// Every `build` tool call stored in a COPY of the app's database becomes a
// corpus case. Its starting world is the world the message before its tool
// result kept (store.js setHeadWorld: every message that was ever the head
// keeps the world as of that point, so the parent of a build's tool-result
// message holds the world the build started from), loaded with
// World.fromManifest from a copy of the chunk files. mc.readFile is served
// from a copy of the chat's /work as the backup left it (the same stand-in for
// every engine); mc.settled reads the starting world itself as the stand-in
// for Minecraft's settle of it, with mc.settledTicks as agent.js computes it.
// Cases stay in the scratch corpus: they are the owner's chats.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { DatabaseSync } = require('node:sqlite');

const W = path.resolve(__dirname, '../../../..');
const { World } = require(path.join(W, 'server/world.js'));
const { encodeWorld } = require(path.join(W, 'server/build-transfer.js'));
const { DEFAULT_SETTLE_TICKS } = require(path.join(W, 'server/minecraft-dimension.js'));

const [dbFile, worldsDir, sandboxDir, blocksFile, corpus] = process.argv.slice(2);
const db = new DatabaseSync(dbFile, { readOnly: true });
fs.mkdirSync(path.join(corpus, 'registries'), { recursive: true });
const registryText = fs.readFileSync(blocksFile, 'utf8');
const registryName = `${crypto.createHash('sha256').update(registryText).digest('hex').slice(0, 16)}.json`;
fs.writeFileSync(path.join(corpus, 'registries', registryName), registryText);

const rows = db.prepare('SELECT conversation_id, id, parent_id, position, payload_json, world_manifest_json FROM messages ORDER BY conversation_id, position').all();
const byId = new Map(rows.map((row) => [row.id, row]));
const calls = new Map();
const stats = { builds: 0, written: 0, noStartWorld: 0, readFile: 0, settled: 0 };
let index = 0;
for (const row of rows) {
  const payload = JSON.parse(row.payload_json);
  for (const call of payload.tool_calls || []) {
    if (call.function?.name === 'build') calls.set(call.id, { message: row, args: JSON.parse(call.function.arguments) });
  }
  if (payload.role !== 'tool' || !calls.has(payload.tool_call_id)) continue;
  stats.builds += 1;
  const { args } = calls.get(payload.tool_call_id);
  const parent = byId.get(row.parent_id);
  if (!parent || !parent.world_manifest_json) { stats.noStartWorld += 1; continue; }
  const conversation = row.conversation_id;
  const chunkDir = path.join(worldsDir, conversation, 'chunks');
  const world = World.fromManifest(JSON.parse(parent.world_manifest_json), (hash) => JSON.parse(fs.readFileSync(path.join(chunkDir, `${hash}.json`), 'utf8')));
  const source = String(args.program || '');
  const recorded = JSON.parse(payload.content);
  index += 1;
  const id = `db-${String(index).padStart(4, '0')}`;
  const dir = path.join(corpus, 'cases', id);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'source.js'), source);
  const worldBytes = encodeWorld(world);
  fs.writeFileSync(path.join(dir, 'world.bin'), worldBytes);
  // agent.js: startTicks = currentTicks(before), the world's settle ticks or the default.
  const ticks = world.settleTicks ?? DEFAULT_SETTLE_TICKS;
  if (/readFile/.test(source)) stats.readFile += 1;
  if (/settled/.test(source)) stats.settled += 1;
  fs.writeFileSync(path.join(dir, 'case.json'), JSON.stringify({
    id,
    origin: 'db',
    conversation,
    message: row.id,
    registry: registryName,
    readFile: null,
    readFileDir: path.join(sandboxDir, conversation),
    settled: { ticks, world: 'world.bin' },
    recorded: { ok: recorded.ok, error: recorded.error ?? null, logs: recorded.logs ?? null },
    resultManifest: row.world_manifest_json ? 'result.json' : null,
  }));
  if (row.world_manifest_json) fs.writeFileSync(path.join(dir, 'result.json'), row.world_manifest_json);
  stats.written += 1;
}
console.log(JSON.stringify(stats));
