'use strict';

// Today's side of mcbench-world's timings (tests/timings.rs is the Rust
// side): each operation on a chat's stored world, timed in-process, once
// after one untimed run, printed as one JSON line of milliseconds.
//
//   node timings.js <checkout> <data folder clone> <chat id> <manifest file to write>

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const [root, data, chat, manifestFile] = process.argv.slice(2);
process.env.MCBENCH_DATA_DIR = data;
const server = (name) => require(path.join(root, 'server', name));
const Database = require(path.join(root, 'node_modules', 'better-sqlite3'));
const { World } = server('world');
const { worldBlueprint } = server('blueprint');
const { encodeWorld, decodeWorld, encodeChanges, applyChanges } = server('build-transfer');

const db = new Database(path.join(data, 'mcbench.sqlite3'), { readonly: true });
const text = db.prepare('SELECT world_manifest_json AS m FROM conversations WHERE id = ?').get(chat).m;
fs.writeFileSync(manifestFile, text);
const folder = path.join(data, 'worlds', chat, 'chunks');
// store.js chunkReader and chunkWriter, without the disk for the writer.
const readChunk = (hash) => JSON.parse(fs.readFileSync(path.join(folder, `${hash}.json`), 'utf8'));
const writeChunk = (pairs) => crypto.createHash('sha256').update(JSON.stringify(pairs)).digest('hex').slice(0, 24);
const normalize = (block) => block;

const ops = {
  load: () => World.fromManifest(JSON.parse(text), readChunk, new Map()),
};
const out = {};
function time(name, setup, fn) {
  for (let run = 0; run < 2; run += 1) {
    const input = setup();
    const started = process.hrtime.bigint();
    fn(input);
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    if (run === 1) out[name] = ms;
  }
}
const base = () => { const world = ops.load(); world.finalize(); world.bounds(); return world; };
const b = base();
const at = [b.bounds().max[0], b.bounds().max[1], b.bounds().max[2]];
time('load', () => null, () => ops.load());
time('clone and one set', () => b, (world) => { const clone = world.clone(); clone.set(at[0], at[1], at[2], 'minecraft:gold_block', {}); });
time('diff', () => { const clone = b.clone(); clone.set(at[0], at[1], at[2], 'minecraft:gold_block', {}); return clone; }, (clone) => clone.diff(b));
time('persist unchanged', () => b, (world) => JSON.stringify(world.serializeManifest(writeChunk)));
time('persist one chunk changed', () => { const clone = b.clone(); clone.set(at[0], at[1], at[2], 'minecraft:gold_block', {}); return clone; }, (clone) => JSON.stringify(clone.serializeManifest(writeChunk)));
time('blueprint', () => b, (world) => worldBlueprint(world, 64));
time('transfer: encode', () => b, (world) => encodeWorld(world));
const sent = encodeWorld(b);
time('transfer: decode', () => sent, (bytes) => decodeWorld(bytes));
time('transfer: to the child and back', () => b, (world) => {
  const start = world.palette.entries.length;
  const child = decodeWorld(encodeWorld(world));
  const changes = encodeChanges(child, child.palette.entries.length);
  applyChanges(world.clone(), changes, start, normalize);
});
time('fill 3M blocks', () => null, () => new World().fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {}));
process.stdout.write(`${JSON.stringify({ side: 'node', chat, blocks: b.size(), ms: out })}\n`);
