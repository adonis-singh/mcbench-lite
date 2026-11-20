'use strict';

// node compare.js <corpus> <node.jsonl> <rust.jsonl> [--examples N] [--json out.json]
//
// Classifies every case's difference between today's runProgram in Node
// (node-run.js) and the Rust runner's inner layer (examples/corpus.rs): the
// change set's bytes, logs, written files, the model-visible error text, and
// whether one side hit the 5 s limit. Prints a table of classes per origin,
// the cases on the allow-list (allow-list.json) with their cause, and
// examples of every other difference.

const fs = require('fs');
const path = require('path');

const [corpus, refFile, engineFile, ...flags] = process.argv.slice(2);
const flag = (name, fallback) => { const i = flags.indexOf(name); return i === -1 ? fallback : flags[i + 1]; };
const examples = Number(flag('--examples', 3));
const jsonOut = flag('--json', null);
const allow = JSON.parse(fs.readFileSync(path.join(__dirname, 'allow-list.json'), 'utf8'));
const read = (file) => new Map(fs.readFileSync(file, 'utf8').trim().split('\n').map((line) => { const o = JSON.parse(line); return [o.id, o]; }));
const ref = read(refFile);
const eng = read(engineFile);
const TIMEOUT = 'Error: Script execution timed out after 5000ms';
// The comparable part of a result, in one key order.
const strip = (o) => (o.ok
  ? { ok: true, changes: o.changes, logs: o.logs, files: o.files, touched: o.touched }
  : { ok: false, error: o.error, logs: o.logs });
const digits = (value) => JSON.stringify(value).replace(/\d+(\.\d+)?/g, '#');

function classify(id, a, b) {
  if (JSON.stringify(strip(a)) === JSON.stringify(strip(b))) return 'identical';
  const source = fs.readFileSync(path.join(corpus, 'cases', id, 'source.js'), 'utf8');
  const aTimeout = a.error === TIMEOUT; const bTimeout = b.error === TIMEOUT;
  if (aTimeout !== bTimeout) return bTimeout ? 'timeout in rust only' : 'timeout in node only';
  if (a.ok && b.ok) {
    if (a.changes !== b.changes) return 'world differs';
    if (JSON.stringify(a.files) !== JSON.stringify(b.files)) return 'written files differ';
    if (/Date\.now|new Date|performance/.test(source) && digits(a.logs) === digits(b.logs)) return 'logs differ only in numbers the program measured with Date';
    return 'logs differ';
  }
  if (a.ok !== b.ok) return a.ok ? 'fails in rust only' : 'fails in node only';
  if (a.error === b.error) {
    if (/Date\.now|new Date|performance/.test(source) && digits(a.logs) === digits(b.logs)) return 'logs differ only in numbers the program measured with Date';
    return 'error logs differ';
  }
  if (/exited unexpectedly|ran out of memory|exceeded \d+ ms/.test(a.error + b.error)) return 'process crash or kill differs';
  const head = (e) => e.split('\n').filter((line) => !/^\s+at /.test(line)).join('\n');
  if (head(a.error) === head(b.error)) return 'error differs in stack frames only';
  const message = (e) => e.split('\n').find((line) => /^[A-Za-z]*Error\b|^[A-Z]\w*: /.test(line)) || e;
  if (message(a.error) === message(b.error)) return 'error differs in source excerpt or caret only';
  return 'error message wording differs';
}

const table = new Map();
const shown = new Map();
const allowed = [];
const unexpected = [];
for (const [id, a] of ref) {
  const b = eng.get(id);
  if (!b) throw new Error(`${id} missing from ${engineFile}`);
  // rec<test file>-N are the recorded test cases.
  const origin = id.split('-')[0].replace(/^rec.*/, 'rec');
  let kind = classify(id, a, b);
  if (kind !== 'identical') {
    const entry = allow[id];
    if (entry && entry.class === kind) {
      allowed.push({ id, kind, cause: entry.cause });
      kind = `allowed: ${kind}`;
    } else if (!(origin === 'db' && kind === 'logs differ only in numbers the program measured with Date')) {
      unexpected.push({ id, kind });
    }
  }
  const key = `${origin}\t${kind}`;
  table.set(key, (table.get(key) || 0) + 1);
  if (kind !== 'identical' && !kind.startsWith('allowed')) {
    const list = shown.get(kind) || [];
    if (list.length < examples) list.push({ id, ref: strip(a), engine: strip(b) });
    shown.set(kind, list);
  }
}
const origins = [...new Set([...table.keys()].map((key) => key.split('\t')[0]))].sort();
const kinds = [...new Set([...table.keys()].map((key) => key.split('\t')[1]))].sort((x, y) => (x === 'identical' ? -1 : y === 'identical' ? 1 : x.localeCompare(y)));
console.log(`| class | ${origins.join(' | ')} | all |`);
console.log(`|---|${origins.map(() => '---:').join('|')}|---:|`);
for (const kind of kinds) {
  const counts = origins.map((origin) => table.get(`${origin}\t${kind}`) || 0);
  console.log(`| ${kind} | ${counts.join(' | ')} | ${counts.reduce((s, c) => s + c, 0)} |`);
}
console.log(`| total | ${origins.map((origin) => [...table].filter(([k]) => k.startsWith(`${origin}\t`)).reduce((s, [, c]) => s + c, 0)).join(' | ')} | ${ref.size} |`);
console.log('\nOn the allow-list:');
for (const { id, kind, cause } of allowed) console.log(`  ${id}: ${kind}: ${cause}`);
const stale = Object.keys(allow).filter((id) => ref.has(id) && !allowed.some((a) => a.id === id));
if (stale.length) console.log(`\nAllow-list entries that did not occur as listed: ${stale.join(' ')}`);
console.log(`\nDifferences outside the allow-list (Date-only logs of db cases excepted): ${unexpected.length}`);
for (const [kind, list] of shown) {
  console.log(`\n### ${kind}`);
  for (const { id, ref: r, engine: e } of list) console.log(`${id}\n  node: ${JSON.stringify(r).slice(0, 900)}\n  rust: ${JSON.stringify(e).slice(0, 900)}`);
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify({ table: Object.fromEntries(table), allowed, unexpected }, null, 1));
