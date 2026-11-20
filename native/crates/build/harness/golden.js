'use strict';

// node golden.js <corpus> <node.jsonl> <out-dir>
//
// Writes the golden files of mcbench-build's tests (tests/golden/) from a
// synthetic corpus and Node 26.13.1's results of it (node-run.js): the
// recorded test cases (record.js over today's tests) and the adversarial
// probes (adversarial.js with golden-registry.json, a test registry, not
// Minecraft's). Cases that read a chat's /work folder (db-*) or a benchmark
// world (bench-*) are left out: the real chats stay in scratch. So are the
// two whose results depend on the machine's time zone (adv-date, adv-intl:
// the child reads /etc/localtime as Node does), which the corpus run checks
// on each machine.
//
//   cases.jsonl   one line per case: id, test, registry, source, world,
//                 settled, readFile, expected (node-run.js's result, the
//                 comparable part)
//   worlds/<sha256 16>.bin.gz   encodeWorld bytes, gzip
//   registries/<name>.json

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');

const [corpus, resultsFile, out] = process.argv.slice(2);
const results = new Map(fs.readFileSync(resultsFile, 'utf8').trim().split('\n').map((line) => { const o = JSON.parse(line); return [o.id, o]; }));
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(path.join(out, 'worlds'), { recursive: true });
fs.mkdirSync(path.join(out, 'registries'), { recursive: true });

const strip = (o) => (o.ok
  ? { ok: true, changes: o.changes, logs: o.logs, files: o.files, touched: o.touched }
  : { ok: false, error: o.error, logs: o.logs });
function world(bytes) {
  const name = crypto.createHash('sha256').update(bytes).digest('hex').slice(0, 16);
  const file = path.join(out, 'worlds', `${name}.bin.gz`);
  if (!fs.existsSync(file)) fs.writeFileSync(file, zlib.gzipSync(bytes, { level: 9 }));
  return name;
}

const TIME_ZONE = new Set(['adv-date', 'adv-intl']);
const lines = [];
for (const id of fs.readdirSync(path.join(corpus, 'cases')).sort()) {
  if (!/^(rec|adv)/.test(id) || TIME_ZONE.has(id)) continue;
  const dir = path.join(corpus, 'cases', id);
  const meta = JSON.parse(fs.readFileSync(path.join(dir, 'case.json'), 'utf8'));
  if (meta.readFileDir) throw new Error(`${id} reads a /work folder`);
  const result = results.get(id);
  if (!result) throw new Error(`${id} has no result in ${resultsFile}`);
  if (result.unstable) throw new Error(`${id} is unstable in Node`);
  fs.copyFileSync(path.join(corpus, 'registries', meta.registry), path.join(out, 'registries', meta.registry));
  lines.push(JSON.stringify({
    id,
    test: meta.test ?? null,
    registry: meta.registry,
    source: fs.readFileSync(path.join(dir, 'source.js'), 'utf8'),
    world: world(fs.readFileSync(path.join(dir, 'world.bin'))),
    settled: meta.settled && { ticks: meta.settled.ticks, world: meta.settled.world ? world(fs.readFileSync(path.join(dir, meta.settled.world))) : null },
    readFile: meta.readFile,
    expected: strip(result),
  }));
}
fs.writeFileSync(path.join(out, 'cases.jsonl'), `${lines.join('\n')}\n`);
console.log(`${lines.length} cases`);
