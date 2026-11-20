'use strict';

// node --expose-gc bench.js <blocks.json> <out-dir> [passes]
//
// The speed table's `build` rows (scripts/benchmark-build.js and
// scripts/benchmark-runtime.js, the speed harness's build-tool row) through
// today's runProgram, sandboxed as shipped, `passes` passes (default 5):
//   emptyWorldMs            one block on an empty world (100 ms pause first)
//   villageMs               the 36-house village on the 256 x 256 terrain
//   oneBlockOnLargeWorldMs  one block on the village's 2,123,612-block world
//   replace3mMs, outline3mMs
// Each also without the parent's World work (encodeWorld of the clone and
// applyChanges of the answer, which mcbench_world does on the Rust side):
// `<row>.protocolMs`. And the cold start the spike measured: spawn of the
// sandboxed child to its result for the one-block program on an empty
// world, no spare process (`coldStartMs`).
// It writes the worlds the Rust side needs (examples/bench.rs) to out-dir:
// terrain.bin, village.bin, stone3m.bin (encodeWorld) and heightmap.json.
// Prints JSON: per row the median and the samples.

const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
const { spawn } = require('child_process');
const { performance } = require('perf_hooks');

const W = process.env.W || path.resolve(__dirname, '../../../..');
const transfer = require(path.join(W, 'server/build-transfer.js'));
let worldMs = 0;
for (const name of ['encodeWorld', 'applyChanges']) {
  const real = transfer[name];
  transfer[name] = function timed(...args) {
    const start = performance.now();
    try { return real.apply(this, args); } finally { worldMs += performance.now() - start; }
  };
}
const sandbox = require(path.join(W, 'server/build-sandbox.js'));
const { runProgram } = sandbox;
const { World } = require(path.join(W, 'server/world.js'));

const [blocksFile, out, passesText = '5'] = process.argv.slice(2);
const registry = JSON.parse(fs.readFileSync(blocksFile, 'utf8'));
const small = { stone: [{}, {}], oak_planks: [{}, {}] };
const bench = fs.readFileSync(path.join(W, 'scripts/benchmark-build.js'), 'utf8');
const SIZE = 256;
const VILLAGE = new Function('SIZE', `return \`${/const VILLAGE = `([\s\S]*?)`;/.exec(bench)[1]}\`;`)(SIZE);
const ONE_BLOCK = /const ONE_BLOCK = '([^']*)'/.exec(bench)[1];
const heights = Array.from({ length: SIZE }, (_, x) => Array.from({ length: SIZE }, (_, z) => 31 + ((x * 7 + z * 13) % 8)));
const heightmap = JSON.stringify(heights);
const REPLACE = 'module.exports = mc => mc.replace("stone", "oak_planks")';
const OUTLINE = 'module.exports = mc => mc.fill([0,0,0], [299,199,49], "stone", "outline")';

const samples = {};
const push = (name, value) => (samples[name] ||= []).push(value);
const pause = () => new Promise((resolve) => setTimeout(resolve, 100));

async function build(name, world, program, reg) {
  await pause();
  const candidate = world.clone();
  worldMs = 0;
  const start = performance.now();
  const result = await runProgram(program, candidate, reg, () => heightmap);
  const ms = performance.now() - start;
  push(name, ms);
  push(`${name}.protocolMs`, ms - worldMs);
  return { candidate, result };
}

function coldStart() {
  const { COMMAND, ARGS, ENV } = sandbox._internal;
  const request = Buffer.from(JSON.stringify({ source: ONE_BLOCK, registry, readFile: false, settledTicks: null }));
  const world = transfer.encodeWorld(new World());
  const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
  const input = Buffer.concat([u32(request.length), request, u32(world.length), world]);
  return new Promise((resolve) => {
    const start = performance.now();
    const child = spawn(COMMAND, ARGS, { stdio: ['pipe', 'pipe', 'pipe'], env: ENV, cwd: '/' });
    child.stdout.once('data', () => { resolve(performance.now() - start); child.kill('SIGKILL'); });
    child.stdin.end(input);
  });
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const terrain = new World();
  terrain.fill([0, 0, 0], [SIZE - 1, 30, SIZE - 1], 'minecraft:stone');
  terrain.fill([0, 0, 0], [SIZE - 1, 4, SIZE - 1], 'minecraft:deepslate');
  terrain.finalize();
  const stone = new World();
  stone.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone');
  stone.finalize();
  let village = null;
  for (let pass = 0; pass < Number(passesText); pass += 1) {
    const v = await build('villageMs', terrain, VILLAGE, registry);
    assert.deepEqual(v.result.logs, ['village 6480 4']);
    v.candidate.finalize();
    assert.equal(v.candidate.size(), 2_123_612);
    village = v.candidate;
    await build('oneBlockOnLargeWorldMs', village, ONE_BLOCK, registry);
    await build('emptyWorldMs', new World(), ONE_BLOCK, registry);
    await build('replace3mMs', stone, REPLACE, small);
    await build('outline3mMs', new World(), OUTLINE, small);
    await pause();
    push('coldStartMs', await coldStart());
    if (global.gc) global.gc();
  }
  fs.writeFileSync(path.join(out, 'terrain.bin'), transfer.encodeWorld(terrain));
  fs.writeFileSync(path.join(out, 'village.bin'), transfer.encodeWorld(village));
  fs.writeFileSync(path.join(out, 'stone3m.bin'), transfer.encodeWorld(stone));
  fs.writeFileSync(path.join(out, 'heightmap.json'), heightmap);
  // Today's child command, for the Rust side's peak-memory runs.
  fs.writeFileSync(path.join(out, 'node-command.json'), JSON.stringify([sandbox._internal.COMMAND, ...sandbox._internal.ARGS]));
  const median = (values) => values.toSorted((a, b) => a - b)[Math.floor(values.length / 2)];
  process.stdout.write(`${JSON.stringify(Object.fromEntries(Object.entries(samples).map(([name, values]) => [name, { median: median(values), samples: values }])))}\n`);
  process.exit(0);
})();
