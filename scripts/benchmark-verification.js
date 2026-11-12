#!/usr/bin/env node
'use strict';

// What a test_build costs on the engines, to size the budgets in
// server/verification-contract.js. Each case is one experiment on a world
// made here, run through EngineVerifier on one engine alone (no routing, so a
// refusal fails the case):
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/benchmark-verification.js
//     --engine rust|native|jvm      rust: core/, native: the built Java engine
//                                   (what the app falls back to), jvm: its classes on GraalVM
//     [--cases regex] [--repeat N]  (default 1; the median is reported)
//     [--json file]                 raw results
//
// The cases vary one thing at a time: operations (probes and samples, with
// and without inventories), loaded columns (a quiet floor of stone), ticks
// per column (quiet and busy content), blocks per column (placement), and the
// largest build the app takes (3,000,000 blocks) on the most columns a test
// may load. Quiet columns are stone; busy columns are rings of observers
// that fire every other tick for the whole run.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { EngineVerifier } = require('../server/minecraft-verify');
const { Engine, coreEngine, jvmEngine } = require('../server/engine');
const { minecraftLive } = require('../server/minecraft-live');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

// Budgets far above the defaults: the benchmark measures, it does not refuse.
const UNLIMITED = { operations: 2_000_000_000, simulationTicks: 2_000_000_000, wallMs: 3_600_000, columns: 100_000, requestBytes: 2_000_000_000, reportBytes: Number.MAX_SAFE_INTEGER };

// A floor of stone `chunks` x `chunks` columns wide and `layers` blocks thick.
function floor(chunks, layers) {
  const world = new World();
  world.fill([0, 0, 0], [chunks * 16 - 1, layers - 1, chunks * 16 - 1], 'minecraft:stone');
  return world;
}

// Rings of four observers, each facing the next: a pulse runs around each
// forever. `layers` layers of 8 x 8 rings per column.
function busyColumns(chunks, layers) {
  const world = new World();
  world.fill([0, 0, 0], [chunks * 16 - 1, 0, chunks * 16 - 1], 'minecraft:stone');
  for (let layer = 0; layer < layers; layer++) {
    const y = 1 + layer;
    for (let x = 0; x < chunks * 16; x += 2) {
      for (let z = 0; z < chunks * 16; z += 2) {
        world.set(x, y, z, 'minecraft:observer', { facing: 'east', powered: 'false' });
        world.set(x + 1, y, z, 'minecraft:observer', { facing: 'south', powered: 'false' });
        world.set(x + 1, y, z + 1, 'minecraft:observer', { facing: 'west', powered: 'false' });
        world.set(x, y, z + 1, 'minecraft:observer', { facing: 'north', powered: 'false' });
      }
    }
  }
  return world;
}

// A line of probes over the floor's top layer and below it, then wrapping
// around when the floor has fewer positions.
function probesOver(count, width, depth, layers) {
  return Array.from({ length: count }, (_, i) => {
    const n = i % (width * depth * layers);
    return [n % width, Math.floor(n / (width * depth)), Math.floor(n / width) % depth];
  });
}

const sequence = (count, step = 1) => Array.from({ length: count }, (_, i) => i * step);

function chests(count, full) {
  const world = new World();
  world.fill([0, 0, 0], [63, 0, 63], 'minecraft:stone');
  const probes = [];
  const block_entities = [];
  for (let i = 0; i < count; i++) {
    const at = [(i % 32) * 2, 1, Math.floor(i / 32) * 2];
    world.set(at[0], at[1], at[2], 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
    probes.push(at);
    if (full) {
      const items = Array.from({ length: 27 }, (_, slot) => `{Slot:${slot}b,id:"minecraft:cobblestone",count:64}`).join(',');
      block_entities.push({ at, data_version: 4556, snbt: `{id:"minecraft:chest",Items:[${items}]}` });
    }
  }
  return { world, probes, block_entities };
}

// The community builds (MCBENCH_CORPUS), as core-verify-parity.js reads them.
function communityBuilds() {
  const builds = new Map(corpusArtifacts());
  return [...builds].sort(([a], [b]) => a.localeCompare(b));
}

const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;

// A community build run for `ticks` ticks: every lever toggled at tick 2, every
// button pressed at tick 4, up to 8 components probed, sampled at 0 and the end.
// With `chunks`, the build is repeated side by side over a square of that many
// chunks (30 is 1,024 columns with the margin, 14 is 256), or as many as the
// app's 3,000,000 blocks hold.
function community(file, ticks, chunks = 0) {
  const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
  const world = new World();
  const probes = [];
  const actions = [];
  const axis = (a) => artifact.blocks.reduce((range, block) => [Math.min(range[0], block.pos[a]), Math.max(range[1], block.pos[a])], [Infinity, -Infinity]);
  const [x0, x1] = axis(0), [z0, z1] = axis(2);
  const stepX = Math.ceil((x1 - x0 + 1) / 16) * 16, stepZ = Math.ceil((z1 - z0 + 1) / 16) * 16;
  const copies = chunks ? { x: Math.floor(chunks * 16 / stepX), z: Math.floor(chunks * 16 / stepZ) } : { x: 1, z: 1 };
  // Within the app's 3,000,000 blocks.
  const limit = Math.floor(2_999_000 / artifact.blocks.length);
  let made = 0;
  for (let cx = 0; cx < copies.x; cx++) {
    for (let cz = 0; cz < copies.z && made < limit; cz++, made++) {
      const shift = (pos) => [pos[0] - x0 + cx * stepX, pos[1], pos[2] - z0 + cz * stepZ];
      for (const block of artifact.blocks) {
        const pos = shift(block.pos);
        world.set(pos[0], pos[1], pos[2], block.id, block.states || {});
        if (COMPONENTS.test(block.id) && probes.length < 8) probes.push(pos);
        if (block.id === 'minecraft:lever') actions.push({ tick: 2, type: 'toggle', at: pos });
        else if (block.id.endsWith('_button')) actions.push({ tick: 4, type: 'press', at: pos });
      }
    }
  }
  if (!probes.length) probes.push(shift0(artifact.blocks[0].pos, x0, z0));
  return { world, args: { probes, actions, samples: [0, ticks] } };
}

const shift0 = (pos, x0, z0) => [pos[0] - x0, pos[1], pos[2] - z0];

// --- The cases ---

function cases() {
  const list = [];
  const add = (name, make) => list.push({ name, make });
  // Operations: probes x (samples + 1), over a floor of 8 + 4 columns, ticks
  // as few as the samples need.
  const bigFloor = () => { const w = new World(); w.fill([0, 0, 0], [63, 3, 31], 'minecraft:stone'); return w; };
  for (const [probes, samples] of [[1, 1], [100, 99], [1000, 99], [10000, 99], [8192, 121], [100, 9999], [83, 11959]]) {
    add(`ops/${probes}p-${samples}s`, () => ({ world: bigFloor(), args: { probes: probesOver(probes, 64, 32, 4), samples: sequence(samples) } }));
  }
  // The same operations as sparse samples (every 10th tick), the usual shape.
  add('ops/8192p-121s-step10', () => ({ world: bigFloor(), args: { probes: probesOver(8192, 64, 32, 4), samples: sequence(121, 10) } }));
  // Inventory probes: each costs 54 operations.
  for (const [count, full, samples] of [[100, false, 100], [100, true, 100], [2000, true, 10]]) {
    add(`inv/${count}chests-${full ? 'full' : 'empty'}-${samples}s`, () => {
      const { world, probes, block_entities } = chests(count, full);
      return { world, args: { probes, inventory_probes: probes, samples: sequence(samples), ...(full ? { block_entities } : {}) } };
    });
  }
  // Columns: setup alone (one tick of settling, one sample), a floor one block thick.
  for (const chunks of [1, 4, 10, 20, 30]) {
    add(`columns/${chunks * chunks}c-floor1`, () => ({ world: floor(chunks, 1), args: { probes: [[0, 0, 0]], samples: [0], settle_ticks: 1 } }));
  }
  // Blocks per column: setup alone, 4 x 4 chunks, thicker floors.
  for (const layers of [1, 4, 16, 64, 128]) {
    add(`blocks/16c-${layers}layers`, () => ({ world: floor(4, layers), args: { probes: [[0, 0, 0]], samples: [0], settle_ticks: 1 } }));
  }
  // Ticks per column, quiet: ticks x columns.
  for (const [chunks, ticks] of [[1, 1000], [4, 1000], [10, 1000], [30, 100], [30, 1000], [30, 3000]]) {
    add(`ticks/${chunks * chunks}c-quiet-${ticks}t`, () => ({ world: floor(chunks, 1), args: { probes: [[0, 0, 0]], samples: [0, ticks], settle_ticks: 1 } }));
  }
  // Ticks per column, busy: observer rings, 1 layer = 256 observers per column.
  for (const [chunks, layers, ticks] of [[1, 1, 1000], [1, 8, 1000], [4, 1, 1000], [4, 8, 1000], [10, 1, 1000], [10, 8, 300]]) {
    add(`ticks/${chunks * chunks}c-busy${layers}-${ticks}t`, () => ({ world: busyColumns(chunks, layers), args: { probes: [[1, 1, 0]], samples: [0, ticks], settle_ticks: 40 } }));
  }
  // Content that random ticks reach (grass spreads, leaves decay, crops grow):
  // 4 x 4 chunks and 30 x 30 chunks.
  for (const [chunks, ticks] of [[4, 1000], [30, 1000]]) {
    add(`ticks/${chunks * chunks}c-random-${ticks}t`, () => {
      const world = floor(chunks, 1);
      world.fill([0, 1, 0], [chunks * 16 - 1, 1, chunks * 16 - 1], 'minecraft:grass_block', { snowy: 'false' });
      world.fill([0, 2, 0], [chunks * 16 - 1, 2, chunks * 16 - 1], 'minecraft:oak_leaves', { distance: '7', persistent: 'false', waterlogged: 'false' });
      return { world, args: { probes: [[0, 0, 0]], samples: [0, ticks], settle_ticks: 1 } };
    });
  }
  // Every community build for 40 and for 1040 ticks: the difference is the
  // cost of 1000 ticks of a real circuit, per loaded column.
  for (const [name, file] of communityBuilds()) {
    add(`corpus-short/${name}`, () => community(file, 20));
    add(`corpus-long/${name}`, () => community(file, 1000));
  }
  // The worst case the budgets allow with real content: the busiest community
  // builds repeated over 1,024 columns, for the ticks asked.
  for (const build of ['OTHERARITH009', 'soontech-LC05-collectors', 'soontech-TP01-foundation']) {
    for (const ticks of [1000, 4000, 12000]) {
      add(`worst/${build}-${ticks}t`, () => community(communityBuilds().find(([name]) => name === build)[1], ticks, 30));
    }
  }
  // Memory and time at many columns: stone floors of about 3,000,000 blocks
  // (the app's limit) spread over 1,024, 4,096 and 12,100 columns, ticking 300 ticks.
  for (const [chunks, layers] of [[30, 13], [62, 3], [108, 1]]) {
    add(`rss/${(chunks + 2) ** 2}c-${layers}layers`, () => ({ world: floor(chunks, layers), args: { probes: [[0, 0, 0]], samples: [0, 300], settle_ticks: 1 } }));
  }
  // 16,384 columns (126 x 126 chunks of build and the margin), a patch of 182 blocks in each chunk.
  add('rss/16384c-sparse', () => {
    const world = new World();
    for (let cx = 0; cx < 126; cx++) for (let cz = 0; cz < 126; cz++) world.fill([cx * 16, 0, cz * 16], [cx * 16 + 12, 0, cz * 16 + 13], 'minecraft:stone');
    return { world, args: { probes: [[0, 0, 0]], samples: [0, 300], settle_ticks: 1 } };
  });
  // A long run: an hour of game time (72,000 ticks) over quiet and over busy content.
  add('long/16c-quiet-72000t', () => ({ world: floor(4, 1), args: { probes: [[0, 0, 0]], samples: [0, 71960], settle_ticks: 40 } }));
  add('long/1c-busy1-72000t', () => ({ world: busyColumns(1, 1), args: { probes: [[1, 1, 0]], samples: [0, 71960], settle_ticks: 40 } }));
  add('long/LC05-72000t', () => community(communityBuilds().find(([name]) => name === 'soontech-LC05-collectors')[1], 71960, 2));
  // The budgets' worst case (server/verification-contract.js): 256 columns for
  // 8,000 ticks (40 of settling and 7,960 of timeline).
  for (const build of ['OTHERARITH009', 'soontech-LC05-collectors', 'soontech-TP01-foundation']) {
    add(`budget/${build}`, () => community(communityBuilds().find(([name]) => name === build)[1], 7960, 14));
  }
  // The largest build, 3,000,000 blocks, on the most columns a test loads:
  // 30 x 30 columns of build plus the margin is 1024. 3,000,000 / 900 columns
  // = 3,333 blocks per column; 13 layers of stone is 2,917 per column.
  add('max/900c-3M-setup', () => ({ world: floor(30, 13), args: { probes: [[0, 0, 0]], samples: [0], settle_ticks: 1 } }));
  add('max/900c-3M-1000t', () => ({ world: floor(30, 13), args: { probes: [[0, 0, 0]], samples: [0, 1000], settle_ticks: 1 } }));
  add('max/1c-3M-ish-busy', () => ({ world: busyColumns(1, 8), args: { probes: [[1, 1, 0]], samples: [0, 1000], settle_ticks: 40 } }));
  return list;
}

async function engineFor(kind, root) {
  if (kind === 'rust') return coreEngine();
  if (kind === 'native') return new Engine();
  if (kind === 'jvm') {
    const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
    const classes = path.join(root, 'classes');
    fs.mkdirSync(classes, { recursive: true });
    return jvmEngine({ classpath, classes });
  }
  throw new Error('--engine must be rust, native or jvm');
}

const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];

async function main() {
  const kind = option('engine', null);
  const match = new RegExp(option('cases', ''));
  const repeat = Number(option('repeat', 1));
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-benchmark-'));
  const verifier = new EngineVerifier({ root, engine: await engineFor(kind, root), budgets: UNLIMITED });
  const rows = [];
  // The engine process's resident memory, sampled while a case runs (kilobytes).
  let peakRss = 0;
  const sampler = setInterval(() => {
    const pid = verifier.engine.child?.pid;
    if (!pid) return;
    try { peakRss = Math.max(peakRss, Number(execFileSync('ps', ['-o', 'rss=', '-p', String(pid)], { encoding: 'utf8' }))); } catch { /* it exited */ }
  }, 200);
  try {
    // The first run starts the engine (and the JVM's code is cold).
    await verifier.run(floor(1, 1), { probes: [[0, 0, 0]], samples: [0, 20] });
    for (const { name, make } of cases().filter((c) => match.test(c.name))) {
      const runs = [];
      let failure = null;
      for (let i = 0; i < repeat && !failure; i++) {
        const { world, args } = make();
        world.finalize();
        const started = Date.now();
        peakRss = 0;
        try {
          const result = await verifier.run(world, args);
          fs.rmSync(result.reportFile, { force: true });
          runs.push({ wallMs: Date.now() - started, simulationMs: result.timings.simulationMs, peakRssMB: Math.round(peakRss / 1024), cost: result.cost, reportBytes: result.reportBytes, blocks: world.size() });
        } catch (error) {
          failure = String(error.message).slice(0, 200);
        }
      }
      const row = failure ? { name, failure } : { name, ...runs[0], wallMs: median(runs.map((r) => r.wallMs)), simulationMs: median(runs.map((r) => r.simulationMs)) };
      rows.push(row);
      console.log(failure ? `${name}: FAILED ${failure}` : `${name}: ${row.simulationMs} ms (wall ${row.wallMs}) ops ${row.cost.operations} ticks ${row.cost.simulationTicks} columns ${row.cost.columns} blocks ${row.blocks} report ${row.reportBytes} bytes, peak RSS ${row.peakRssMB} MB`);
    }
  } finally {
    clearInterval(sampler);
    await verifier.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
  // Per loaded column and tick of the community builds: the 1000-tick run less
  // the 20-tick run, over 980 ticks and the build's columns.
  const short = new Map(rows.filter((r) => !r.failure && r.name.startsWith('corpus-short/')).map((r) => [r.name.slice(13), r]));
  const perColumnTick = rows.filter((r) => !r.failure && r.name.startsWith('corpus-long/') && short.has(r.name.slice(12))).map((r) => {
    const base = short.get(r.name.slice(12));
    return { name: r.name.slice(12), columns: r.cost.columns, blocks: r.blocks, ms: r.simulationMs, usPerColumnTick: 1000 * Math.max(0, r.simulationMs - base.simulationMs) / (980 * r.cost.columns) };
  }).sort((a, b) => b.usPerColumnTick - a.usPerColumnTick);
  if (perColumnTick.length) {
    console.log('community builds by cost per loaded column and tick (microseconds):');
    for (const row of perColumnTick.slice(0, 12)) console.log(`  ${row.name}: ${row.usPerColumnTick.toFixed(2)} (${row.columns} columns, ${row.blocks} blocks, ${row.ms} ms for 1000 ticks)`);
    const sorted = perColumnTick.map((r) => r.usPerColumnTick).sort((a, b) => a - b);
    console.log(`  median ${sorted[Math.floor(sorted.length / 2)].toFixed(2)}, p90 ${sorted[Math.floor(sorted.length * 0.9)].toFixed(2)}, max ${sorted.at(-1).toFixed(2)}`);
  }
  if (option('json', null)) fs.writeFileSync(option('json'), JSON.stringify({ engine: kind, cpu: os.cpus()[0]?.model, cpus: os.cpus().length, memory: os.totalmem(), rows }, null, 2));
}

main().then(() => process.exit(0), (error) => { console.error(error); process.exit(1); });
