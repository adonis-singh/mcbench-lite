'use strict';

// The differential harness of mcbench-build (ported from the JavaScript
// spike's harness/, native/spikes/js/harness on claude/sharp-mendeleev-e55a37).
//
// node node-run.js <corpus> <out.jsonl> [repeat] [id-substring]
//
// Runs every corpus case through today's parent, server/build-sandbox.js
// runProgram, with its own sandboxed `node build-child.js` (the Node this
// script runs on), and agent.js's model-facing formatting (formatProgramError
// and the 8,000-character cut). One build at a time as the app does, with a
// pause between builds standing in for the model's turn (the spare process
// starts in it). Writes one JSON line per case: the first run's result and
// every run's milliseconds; a run whose result differs from the first is
// `unstable`. The change set the child sent back is captured as it reaches
// build-transfer.js applyChanges, so a result names its bytes (`changes`,
// the sha256 of them) as the Rust runner's inner layer returns them
// (examples/corpus.rs writes the same lines; compare.js classifies).
//
// Corpus layout (record.js, adversarial.js, extract-db.js write it):
//   cases/<id>/{case.json, source.js, world.bin[, settled.bin]}
//   registries/<sha16>.json
// case.json: { id, registry, readFile: {path: {ok, text}} | null,
//              readFileDir?: <folder of the chat's /work>,
//              settled: { ticks, world: 'world.bin' | 'settled.bin' | null } | null }

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const W = process.env.W || path.resolve(__dirname, '../../../..');
const transfer = require(path.join(W, 'server/build-transfer.js'));
let captured = null;
const realApply = transfer.applyChanges;
transfer.applyChanges = function applyChanges(world, buffer, ...rest) {
  captured = Buffer.from(buffer);
  return realApply.call(this, world, buffer, ...rest);
};
const { runProgram } = require(path.join(W, 'server/build-sandbox.js'));
const { formatProgramError } = require(path.join(W, 'server/mc-dsl.js'));
const { World } = require(path.join(W, 'server/world.js'));

const MAX_ERROR_CHARS = 8000;
const sha = (data) => crypto.createHash('sha256').update(data).digest('hex');

const registries = new Map();
function registryOf(corpus, name) {
  const file = path.join(corpus, 'registries', name);
  if (!registries.has(file)) registries.set(file, JSON.parse(fs.readFileSync(file, 'utf8')));
  return registries.get(file);
}

function loadCase(corpus, id) {
  const dir = path.join(corpus, 'cases', id);
  const meta = JSON.parse(fs.readFileSync(path.join(dir, 'case.json'), 'utf8'));
  return {
    ...meta,
    source: fs.readFileSync(path.join(dir, 'source.js'), 'utf8'),
    registry: registryOf(corpus, meta.registry),
    worldBytes: fs.readFileSync(path.join(dir, 'world.bin')),
    settledBytes: meta.settled && meta.settled.world ? fs.readFileSync(path.join(dir, meta.settled.world)) : null,
  };
}

// The readFile the case's host had: its recorded answers, or a copy of the
// chat's /work folder (READFILE_ROOT=<from>=<to> moves it), the same for
// both runtimes (examples/corpus.rs serves it the same way).
function readFileOf(c) {
  if (c.readFileDir) {
    const [from, to] = (process.env.READFILE_ROOT || '=').split('=');
    const root = from ? c.readFileDir.replace(from, to) : c.readFileDir;
    return (relPath) => {
      const file = path.resolve(root, relPath.replace(/^\/work\//, ''));
      if (!file.startsWith(`${root}/`)) throw new Error(`mc.readFile: ${relPath} is outside /work`);
      try { return fs.readFileSync(file, 'utf8'); } catch (error) { throw new Error(`mc.readFile: ${relPath}: ${error.code}`); }
    };
  }
  if (!c.readFile) return null;
  return (relPath) => {
    const answer = c.readFile[relPath];
    if (!answer) throw new Error(`mc.readFile: ${relPath}: no such file in /work`);
    if (!answer.ok) throw new Error(answer.text);
    return answer.text;
  };
}

async function runCase(c) {
  const { decodeWorld } = transfer;
  const world = decodeWorld(c.worldBytes);
  const minecraft = c.settled ? { world: c.settledBytes ? decodeWorld(c.settledBytes) : new World(), ticks: c.settled.ticks } : null;
  captured = null;
  const started = process.hrtime.bigint();
  let out;
  try {
    const { logs, files, touched } = await runProgram(c.source, world, c.registry, readFileOf(c), minecraft);
    out = { ok: true, changes: sha(captured), logs, files: files.map((f) => ({ path: f.path, length: f.text.length, sha: sha(f.text) })), touched };
  } catch (err) {
    const error = formatProgramError(err);
    out = {
      ok: false,
      error: error.length > MAX_ERROR_CHARS ? `${error.slice(0, MAX_ERROR_CHARS)}\n[error message cut at ${MAX_ERROR_CHARS} characters]` : error,
      logs: err?.logs?.length ? err.logs : null,
    };
  }
  out.ms = Number(process.hrtime.bigint() - started) / 1e6;
  return out;
}

const PAUSE_MS = 100;
const pause = () => new Promise((resolve) => setTimeout(resolve, PAUSE_MS));

if (require.main === module) {
  const [corpus, outFile, repeatText = '1', filter = ''] = process.argv.slice(2);
  const repeat = Number(repeatText);
  (async () => {
    const ids = fs.readdirSync(path.join(corpus, 'cases')).filter((id) => id.includes(filter)).sort();
    const out = fs.openSync(outFile, 'w');
    let done = 0;
    for (const id of ids) {
      const c = loadCase(corpus, id);
      const runs = [];
      let first = null;
      let unstable = false;
      for (let i = 0; i < repeat; i += 1) {
        await pause();
        const result = await runCase(c);
        runs.push(result.ms);
        delete result.ms;
        if (!first) first = result;
        else if (JSON.stringify(result) !== JSON.stringify(first)) unstable = true;
      }
      fs.writeSync(out, `${JSON.stringify({ id, ...first, unstable, ms: runs })}\n`);
      done += 1;
      if (done % 50 === 0) process.stderr.write(`node: ${done}/${ids.length}\n`);
    }
    fs.closeSync(out);
    process.exit(0);
  })();
}

module.exports = { runCase, loadCase, readFileOf };
