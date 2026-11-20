'use strict';

// The Node side of mcbench-world's fuzz (tests/fuzz.rs): random operation
// sequences from one seed, run on today's World, every return value and
// error message written down with the operations, for the Rust side to run
// the same operations and compare.
//
//   node fuzz.js <checkout> <out file> <seed> <sequences>

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const [root, outFile, seedText, countText] = process.argv.slice(2);
const server = (name) => require(path.join(root, 'server', name));
const { World, MAX_COORD } = server('world');
const { worldBlueprint } = server('blueprint');
const { encodeWorld, encodeChanges } = server('build-transfer');

const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
let seed = Number(seedText) >>> 0;
const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 2 ** 32; };
const int = (lo, hi) => lo + Math.floor(random() * (hi - lo + 1));
const pick = (list) => list[Math.floor(random() * list.length)];

const IDS = ['minecraft:stone', 'minecraft:stone', 'minecraft:oak_planks', 'minecraft:air', 'minecraft:air', 'minecraft:oak_stairs', 'minecraft:glass', 'not a block', '', 'minecraft:ünïcode', 'test:"quoted"\\'];
const STATES = [
  [], [], [['facing', 'east']], [['half', 'top'], ['facing', 'east']], [['facing', 'east'], ['half', 'top']],
  [['10', 'x'], ['2', 'y'], ['b', 'z']], [['a', '1'], ['a', '2']], [['lit', 'true']], [['level', '\n\u0001é']],
];
const objectOf = (pairs) => { const out = {}; for (const [k, v] of pairs) out[k] = v; return out; };
const pairsOf = (object) => Object.entries(object);
const attempt = (fn) => { try { const value = fn(); return value === undefined ? {} : { value }; } catch (error) { return { error: String(error.message) }; } };

function coord(spread) {
  const r = random();
  if (r < 0.03) return pick([MAX_COORD, -MAX_COORD, MAX_COORD + 1, -MAX_COORD - 1, MAX_COORD - 1]);
  return int(-spread, spread);
}
const at = (spread) => [coord(spread), coord(spread), coord(spread)];
// Boxes have min at most max on every axis, as mc-dsl.js regionCorners
// makes them: a box with min past max leaves an empty chunk in today's world
// (writeBox makes the chunk before its empty loop) and its bounds() then
// throws, a state no caller reaches.
const box = (spread, size) => { const min = at(spread); const max = min.map((v) => v + int(0, size)); return [min, max]; };

function recorder() {
  const files = [];
  return { files, write: (pairs) => { const payload = JSON.stringify(pairs); const hash = sha(payload).slice(0, 24); files.push(`${hash} ${sha(payload)}`); return hash; } };
}
const manifestOf = (world) => { const r = recorder(); const text = JSON.stringify(world.serializeManifest(r.write)); return { manifest: sha(text), files: sha(r.files.join('\n')), written: r.files.length }; };
const entryOf = (entry) => (entry ? { id: entry.id, states: entry.states } : null);

function sequence(index) {
  const kind = index % 100 === 7 ? 'limit' : index === 13 ? 'palette' : 'plain';
  const spread = pick([8, 20, 40, 300]);
  const ops = [];
  let world = new World();
  let origin = null;
  let paletteStart = 1;
  const run = (op, fn) => { op.result = attempt(fn); ops.push(op); };
  if (kind === 'limit') {
    run({ op: 'fill', min: [0, 0, 0], max: [299, 199, 49], id: 'minecraft:stone', states: [] }, () => world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {}));
    const op = { op: 'fill', min: [0, 0, 0], max: [int(0, 9), 0, 0], id: 'minecraft:air', states: [] };
    run(op, () => world.fill(op.min, op.max, op.id, {}));
    run({ op: 'finalize' }, () => { world.finalize(); });
    // Past the room left, inside it, and room made by a clear in the same call.
    for (const [min, max, clear] of [[[290, 0, 0], [310, 1, 0], []], [[0, 0, 50], [3, 0, 50], [[[0, 0, 0], [3, 0, 0]]]], [[0, 0, 50], [int(0, 12), 0, 50], []], [[300, 0, 0], [300, 0, 0], [[[10, 0, 0], [10, 0, 0]]]]]) {
      const fillOp = { op: 'fillBoxes', parts: [[min, max]], clear, id: pick(['minecraft:oak_planks', 'minecraft:air']), states: [] };
      run(fillOp, () => world.fillBoxes(fillOp.parts, fillOp.id, {}, fillOp.clear));
    }
  }
  if (kind === 'palette') {
    for (let i = 0; i <= 65536; i += 1) {
      const op = { op: 'set', at: [i % 64, Math.floor(i / 4096), Math.floor(i / 64) % 64], id: 'minecraft:stone', states: [['n', String(i)]] };
      run(op, () => world.set(...op.at, op.id, objectOf(op.states)));
    }
  }
  const length = int(10, 60);
  for (let step = 0; step < length; step += 1) {
    const r = random();
    if (r < 0.30) {
      const op = { op: 'set', at: at(spread), id: pick(IDS), states: pick(STATES) };
      run(op, () => world.set(...op.at, op.id, objectOf(op.states)));
    } else if (r < 0.42) {
      const [min, max] = box(spread, pick([1, 4, 9, 20, 40]));
      const op = { op: 'fill', min, max, id: pick(IDS), states: pick(STATES) };
      run(op, () => world.fill(op.min, op.max, op.id, objectOf(op.states)));
    } else if (r < 0.46) {
      const parts = [box(spread, 6)];
      const clear = random() < 0.5 ? [box(spread, 6)] : [];
      const op = { op: 'fillBoxes', parts, clear, id: pick(IDS), states: pick(STATES) };
      run(op, () => world.fillBoxes(op.parts, op.id, objectOf(op.states), op.clear));
    } else if (r < 0.56) {
      const op = { op: 'get', at: at(spread) };
      run(op, () => ({ entry: entryOf(world.get(...op.at)), has: world.has(...op.at) }));
    } else if (r < 0.62) {
      run({ op: 'finalize' }, () => { world.finalize(); });
    } else if (r < 0.68) {
      run({ op: 'clone' }, () => { const next = world.clone(); origin = world; world = next; paletteStart = world.palette.entries.length; });
    } else if (r < 0.72) {
      if (origin) run({ op: 'diff' }, () => world.diff(origin));
    } else if (r < 0.78) {
      run({ op: 'look' }, () => ({ size: world.size(), bounds: world.bounds(), chunks: [...world.chunks].map(([key, chunk]) => [key, chunk.count, Boolean(chunk.dense)]), changed: [...world.changedChunks], owned: [...world.ownedChunks], near: [...world.nearChangedChunks()], palette: world.palette.entries.length, lod: world.renderLod() }));
    } else if (r < 0.81) {
      const op = { op: 'replace', id: pick(IDS), target: pick(IDS), box: random() < 0.5 ? null : (([min, max]) => ({ min, max }))(box(spread, 20)) };
      run(op, () => world.replace(op.id, (entry) => ({ id: op.target, states: entry.states }), op.box));
    } else if (r < 0.84) {
      run({ op: 'clear' }, () => { world.clear(); });
    } else if (r < 0.88) {
      run({ op: 'encode' }, () => ({ world: sha(encodeWorld(world)), changes: sha(encodeChanges(world, Math.min(paletteStart, world.palette.entries.length))) }));
    } else if (r < 0.91) {
      const op = { op: 'blueprint', y: pick([64, 0, 7]) };
      run(op, () => sha(worldBlueprint(world, op.y)));
    } else if (r < 0.94) {
      run({ op: 'manifest' }, () => manifestOf(world));
    } else {
      run({ op: 'reference' }, () => { world = world.reference(); origin = null; });
    }
  }
  const final = attempt(() => manifestOf(world));
  return { kind, ops, final, entries: world.palette.entries.slice(1).map((e) => [e.id, pairsOf(e.states)]) };
}

const sequences = [];
for (let index = 0; index < Number(countText); index += 1) sequences.push(sequence(index));
fs.writeFileSync(outFile, JSON.stringify({ seed: Number(seedText), sequences }));
process.stdout.write(`${sequences.length} sequences, ${sequences.reduce((n, s) => n + s.ops.length, 0)} operations\n`);
