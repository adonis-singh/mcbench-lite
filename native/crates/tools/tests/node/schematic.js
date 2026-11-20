'use strict';

// Today's schematic of every stored world of a data folder (each chat's
// current world and each message's world), for examples/schematic.rs to
// compare: one line per world, the sha256 of writeSchematic's bytes (gzip)
// and of the NBT inside, or the error.
//
//   node schematic.js <data folder clone> <out.jsonl>

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');
const Database = require('better-sqlite3');

const root = path.resolve(__dirname, '../../../../..');
const [dataDir, outFile] = process.argv.slice(2);
const { World } = require(path.join(root, 'server/world.js'));
const { writeSchematic } = require(path.join(root, 'server/schematic.js'));

const db = new Database(path.join(dataDir, 'mcbench.sqlite3'), { readonly: true, fileMustExist: true });
const rows = [
  ...db.prepare('SELECT id AS owner, id, world_manifest_json AS manifest FROM conversations').all(),
  ...db.prepare('SELECT conversation_id AS owner, id, world_manifest_json AS manifest FROM messages WHERE world_manifest_json IS NOT NULL').all(),
];
const seen = new Set();
const out = fs.openSync(outFile, 'w');
const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
for (const row of rows) {
  if (seen.has(row.manifest)) continue;
  seen.add(row.manifest);
  const chunks = path.join(dataDir, 'worlds', row.owner, 'chunks');
  const world = World.fromManifest(JSON.parse(row.manifest), (hash) => JSON.parse(fs.readFileSync(path.join(chunks, `${hash}.json`), 'utf8')));
  let line;
  try {
    const bytes = writeSchematic(world);
    line = { key: `${row.owner}/${row.id}`, owner: row.owner, manifest: row.manifest, gzip: sha(bytes), nbt: sha(zlib.gunzipSync(bytes)), bytes: bytes.length };
  } catch (error) {
    line = { key: `${row.owner}/${row.id}`, owner: row.owner, manifest: row.manifest, error: error.message };
  }
  fs.writeSync(out, `${JSON.stringify(line)}\n`);
}
fs.closeSync(out);
console.log(JSON.stringify({ worlds: seen.size }));
