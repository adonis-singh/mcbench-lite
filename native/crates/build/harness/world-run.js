'use strict';

// node world-run.js <corpus> <store> <out.jsonl> [id-substring]
//
// The world-level differential (Node side; examples/world_run.rs is the
// Rust side): every case's starting world (and settled world) is stored
// once under <store>/<id>/ as today's store keeps a chat's world (a
// manifest, JSON.stringify of serializeManifest, and chunk files named by
// the first 24 hex digits of their sha256), then loaded back with
// World.fromManifest, cloned and run through today's runProgram. A world a
// program changed is finalized and persisted as agent.js persists it; the
// result names the persisted world as `changes`: the sha256 of the manifest
// text and of every chunk file's bytes in the manifest's order. Lines are
// node-run.js's, so compare.js classifies them.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const W = process.env.W || path.resolve(__dirname, '../../../..');
const { runProgram } = require(path.join(W, 'server/build-sandbox.js'));
const { formatProgramError } = require(path.join(W, 'server/mc-dsl.js'));
const { decodeWorld } = require(path.join(W, 'server/build-transfer.js'));
const { World } = require(path.join(W, 'server/world.js'));
const { loadCase, readFileOf } = require('./node-run.js');

const MAX_ERROR_CHARS = 8000;
const sha = (data) => crypto.createHash('sha256').update(data).digest('hex');

// store.js chunkWriter, into `files` (hash -> payload) and, when given, a folder.
function writer(files, dir) {
  return (blocks) => {
    const payload = JSON.stringify(blocks);
    const hash = sha(payload).slice(0, 24);
    files.set(hash, payload);
    if (dir) {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, `${hash}.json`), payload);
    }
    return hash;
  };
}

function stored(dir, name, bytes) {
  const file = path.join(dir, `${name}.json`);
  if (!fs.existsSync(file)) {
    fs.mkdirSync(dir, { recursive: true });
    const manifest = decodeWorld(bytes).serializeManifest(writer(new Map(), path.join(dir, 'chunks')));
    fs.writeFileSync(file, JSON.stringify(manifest));
  }
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

const load = (dir, manifest) => World.fromManifest(manifest, (hash) => JSON.parse(fs.readFileSync(path.join(dir, 'chunks', `${hash}.json`), 'utf8')));

async function runCase(c, dir) {
  const before = load(dir, stored(dir, 'manifest', c.worldBytes));
  const settled = c.settled ? (c.settledBytes ? load(dir, stored(dir, 'settled', c.settledBytes)) : new World()) : null;
  const candidate = before.clone();
  const started = process.hrtime.bigint();
  let out;
  try {
    const { logs, files, touched } = await runProgram(c.source, candidate, c.registry, readFileOf(c), settled && { world: settled, ticks: c.settled.ticks });
    candidate.finalize();
    const written = new Map();
    const manifest = JSON.stringify(candidate.serializeManifest(writer(written)));
    const chunkBytes = JSON.parse(manifest).chunks.map(([, hash]) => written.get(hash) ?? fs.readFileSync(path.join(dir, 'chunks', `${hash}.json`), 'utf8'));
    out = { ok: true, changes: `${sha(manifest)}:${sha(chunkBytes.join('\n'))}`, logs, files: files.map((f) => ({ path: f.path, length: f.text.length, sha: sha(f.text) })), touched };
  } catch (err) {
    const error = formatProgramError(err);
    out = { ok: false, error: error.length > MAX_ERROR_CHARS ? `${error.slice(0, MAX_ERROR_CHARS)}\n[error message cut at ${MAX_ERROR_CHARS} characters]` : error, logs: err?.logs?.length ? err.logs : null };
  }
  out.ms = Number(process.hrtime.bigint() - started) / 1e6;
  return out;
}

const [corpus, store, outFile, filter = ''] = process.argv.slice(2);
(async () => {
  const ids = fs.readdirSync(path.join(corpus, 'cases')).filter((id) => id.includes(filter)).sort();
  const out = fs.openSync(outFile, 'w');
  for (const [done, id] of ids.entries()) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    const result = await runCase(loadCase(corpus, id), path.join(store, id));
    const ms = result.ms;
    delete result.ms;
    fs.writeSync(out, `${JSON.stringify({ id, ...result, unstable: false, ms: [ms] })}\n`);
    if ((done + 1) % 50 === 0) process.stderr.write(`node: ${done + 1}/${ids.length}\n`);
  }
  fs.closeSync(out);
  process.exit(0);
})();
