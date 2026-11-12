#!/usr/bin/env node
'use strict';

// What a settle costs by the ticks it runs, on stored builds' worlds: the
// engine run (shot-world.js settle()) for 0, 100, 210 and 1000 ticks, each a
// run of its own, and for four ticks asked at once (one run, the world
// written at each) against the same four asked one by one. Medians over --runs.
// Each figure is the median / the fastest of the runs (the fastest is the least
// disturbed by whatever else the machine runs).
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/benchmark-ticks.js [--runs 5] [--java] <conversation id>...
// --java adds a block the Rust engine refuses (resin_clump), so the Java
// engine runs the ticks.
// Wrap in the native-engine lock: until mkdir /tmp/mcbench-native.lock; do sleep 30; done

const { performance } = require('node:perf_hooks');
const store = require('../server/store');
const { settle, stopShotEngines } = require('../server/shot-world');

const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const round = (value) => Math.round(value * 10) / 10;
const both = (values) => `${round(median(values))} / ${round(Math.min(...values))}`;
let key = 0;

// Milliseconds of one settle of `world` for `ticks`, on a conversation of its own (nothing cached).
async function time(world, ticks) {
  const started = performance.now();
  const results = await settle(`benchmark-${key++}`, world, ticks);
  return { ms: performance.now() - started, timings: results.get(ticks.at(-1)).timings };
}

async function main() {
  const args = process.argv.slice(2);
  const runsAt = args.indexOf('--runs');
  const runs = runsAt === -1 ? 5 : Number(args.splice(runsAt, 2)[1]);
  const java = args.includes('--java');
  for (const id of args.filter((arg) => !arg.startsWith('--'))) {
    const conversation = store.getConversation(id);
    let world = conversation.world;
    if (java) {
      world = world.clone();
      const bounds = world.bounds();
      world.set(bounds.min[0], bounds.max[1] + 1, bounds.min[2], 'minecraft:resin_clump');
      world.finalize();
    }
    const row = { conversation: id.slice(0, 8), title: conversation.title, blocks: world.size(), engine: java ? 'java' : 'rust', ms: {} };
    await time(world, [0]);
    for (const ticks of [0, 100, 210, 1000]) {
      const samples = [];
      for (let i = 0; i < runs; i += 1) samples.push(await time(world, [ticks]));
      row.ms[ticks] = both(samples.map((s) => s.ms));
      row.fastest ||= {};
      row.fastest[ticks] = round(Math.min(...samples.map((s) => s.ms)));
      if (ticks === 0) row.engineRun = Object.fromEntries(['commands', 'place', 'read', 'compare'].map((name) => [name, round(median(samples.map((s) => s.timings[name] || 0)))]));
    }
    row.perHundredTicks = round((row.fastest[1000] - row.fastest[100]) / 9);
    const together = [];
    const apart = [];
    for (let i = 0; i < runs; i += 1) {
      together.push((await time(world, [0, 30, 100, 210])).ms);
      let total = 0;
      for (const ticks of [0, 30, 100, 210]) total += (await time(world, [ticks])).ms;
      apart.push(total);
    }
    row.fourSnapshots = { oneRun: both(together), fourRuns: both(apart) };
    console.log(JSON.stringify(row));
  }
  await stopShotEngines();
  store.close();
}

main().catch((error) => { console.error(error); process.exit(1); });
