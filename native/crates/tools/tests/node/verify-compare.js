'use strict';

// Compares what the two sides of mcbench-tools' verifier differential wrote:
// <out>/node/ (verify-node.js) and <out>/<side>/ (the Rust test, or the
// headless grade client), case by case: the result JSON byte for byte and
// every report file byte for byte, after the run-dependent fields are masked
// on both sides alike:
//
//   wallMs, timings.*          wall-clock times of the run
//   metrics.probeReadMs,       the engine's own timings in the result and
//   metrics.maxSampleReadMs    the report
//   reportBytes                the report's length, which holds those timings
//   report, reportFile         paths named by random UUIDs or by the side
//   a wallMs budget error's    how far the run got before its wall-clock
//   requested, phase,          budget ended it, in its budget and its message
//   ticksReached, totalTicks
//
//   node verify-compare.js <out folder> <side> [--json file] [--kind call|grade]
//
// Prints a table by case group and every difference; exits 1 when there is
// one. --kind compares the cases of that kind only.

const fs = require('fs');
const path = require('path');

const [out, side] = process.argv.slice(2);
if (!side) throw new Error('usage: node verify-compare.js <out folder> <side> [--json file] [--kind call|grade]');
const option = (name) => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : null);
const json = option('--json');
const kind = option('--kind');

const WALL = /it (?:had not started ticking yet|was still settling, at tick \d+ of \d+ settling ticks|reached tick \d+ of \d+ of its timeline, after its settling ticks)\./g;
function norm(value) {
  if (Array.isArray(value)) return value.map(norm);
  if (value && typeof value === 'object') {
    const masked = {};
    const wall = value.resource === 'wallMs';
    for (const [key, item] of Object.entries(value)) {
      if (key === 'wallMs' && typeof item === 'number') masked[key] = '<ms>';
      else if (key === 'timings' && item && typeof item === 'object') masked[key] = Object.fromEntries(Object.entries(item).map(([k, v]) => [k, typeof v === 'number' ? '<ms>' : v]));
      else if ((key === 'probeReadMs' || key === 'maxSampleReadMs') && typeof item === 'number') masked[key] = '<ms>';
      else if (key === 'reportBytes' && typeof item === 'number') masked[key] = '<bytes>';
      else if (key === 'report' && typeof item === 'string' && /^\/work\/verification\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.json$/.test(item)) masked[key] = '<report>';
      else if (key === 'reportFile' && typeof item === 'string') masked[key] = '<reportFile>';
      else if (wall && ['requested', 'phase', 'ticksReached', 'totalTicks'].includes(key)) masked[key] = '<reached>';
      else masked[key] = norm(item);
    }
    return masked;
  }
  if (typeof value === 'string') return value.replace(WALL, 'it <where>.');
  return value;
}
const masked = (text) => JSON.stringify(norm(JSON.parse(text)));

function firstDifference(a, b) {
  let i = 0;
  while (i < a.length && a[i] === b[i]) i++;
  return `at ${i}: node …${a.slice(Math.max(0, i - 80), i + 80)}… / ${side} …${b.slice(Math.max(0, i - 80), i + 80)}…`;
}

const cases = fs.readFileSync(path.join(out, 'cases.jsonl'), 'utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line));
const groups = new Map();
const differences = [];
let identicalRaw = 0;
for (const [n, c] of cases.entries()) {
  if (kind && c.kind !== kind) continue;
  const group = c.name.split('/')[0];
  const row = groups.get(group) || { cases: 0, identical: 0, completed: 0, failed: 0, files: 0, different: 0 };
  groups.set(group, row);
  row.cases++;
  const read = (who) => {
    const file = path.join(out, who, `${n}.json`);
    return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
  };
  const node = read('node');
  const other = read(side);
  const problems = [];
  if (node === null || other === null) problems.push(`missing output (node ${node !== null}, ${side} ${other !== null})`);
  else {
    if (node === other) identicalRaw++;
    const a = masked(node), b = masked(other);
    if (a !== b) problems.push(`result ${firstDifference(a, b)}`);
    const value = JSON.parse(node);
    const tests = c.kind === 'call' ? value.tests || [] : [value];
    tests.forEach((t) => { if (t.ok) row.completed++; else row.failed++; });
    const names = (who) => (fs.existsSync(path.join(out, who, String(n))) ? fs.readdirSync(path.join(out, who, String(n))).sort() : []);
    const nodeFiles = names('node'), otherFiles = names(side);
    if (nodeFiles.join() !== otherFiles.join()) problems.push(`report files: node ${nodeFiles.join(',')} / ${side} ${otherFiles.join(',')}`);
    else for (const name of nodeFiles) {
      row.files++;
      const x = fs.readFileSync(path.join(out, 'node', String(n), name), 'utf8');
      const y = fs.readFileSync(path.join(out, side, String(n), name), 'utf8');
      if (x !== y && masked(x) !== masked(y)) problems.push(`report ${name} ${firstDifference(masked(x), masked(y))}`);
    }
  }
  if (problems.length) {
    row.different++;
    differences.push({ n, name: c.name, problems });
  } else row.identical++;
}

console.log('| Group | Cases | Identical | Tests completed | Tests failed (same error) | Report files | Different |');
console.log('|---|---|---|---|---|---|---|');
const total = { cases: 0, identical: 0, completed: 0, failed: 0, files: 0, different: 0 };
for (const [group, row] of groups) {
  console.log(`| ${group} | ${row.cases} | ${row.identical} | ${row.completed} | ${row.failed} | ${row.files} | ${row.different} |`);
  for (const key of Object.keys(total)) total[key] += row[key];
}
console.log(`| all | ${total.cases} | ${total.identical} | ${total.completed} | ${total.failed} | ${total.files} | ${total.different} |`);
console.log(`${identicalRaw} of ${total.cases} results identical before masking`);
for (const d of differences) console.log(`DIFFERENT ${d.n} ${d.name}: ${d.problems.join(' | ').slice(0, 1200)}`);
if (json) fs.writeFileSync(json, JSON.stringify({ groups: Object.fromEntries(groups), total, identicalRaw, differences }, null, 2));
process.exit(differences.length ? 1 : 0);
