#!/usr/bin/env node
'use strict';

// How fast several conversations settle at once: --count conversations, each
// with the stored build's world, asked together for --ticks ticks, on a pool
// of MCBENCH_SETTLE_ENGINES engines. Prints the wall time and the settles a
// second, and a digest of every result (the same for any pool size).
//   MCBENCH_SETTLE_ENGINES=4 MCBENCH_DATA_DIR=<scratch copy> node scripts/benchmark-pool.js [--count 8] [--ticks 210] [--runs 3] <conversation id>
// Wrap in the native-engine lock: until mkdir /tmp/mcbench-native.lock; do sleep 30; done

const crypto = require('node:crypto');
const { performance } = require('node:perf_hooks');
const store = require('../server/store');
const { settle, stopShotEngines } = require('../server/shot-world');
const { poolSize } = require('../server/engine-pool');

function digest(result) {
  const hash = crypto.createHash('sha256');
  const blocks = [];
  result.world.forEach((x, y, z, entry) => blocks.push(`${x},${y},${z} ${entry.id}${JSON.stringify(entry.states)}`));
  hash.update(blocks.sort().join('\n'));
  hash.update(JSON.stringify(result.changes));
  return hash.digest('hex').slice(0, 12);
}

async function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback) => { const at = args.indexOf(name); return at === -1 ? fallback : Number(args.splice(at, 2)[1]); };
  const count = option('--count', 8);
  const ticks = option('--ticks', 210);
  const runs = option('--runs', 3);
  const conversation = store.getConversation(args[0]);
  const world = conversation.world;
  let run = 0;
  await settle('benchmark-warm', world, [0]);
  const walls = [];
  let digests;
  for (let i = 0; i < runs; i += 1) {
    const started = performance.now();
    const results = await Promise.all(Array.from({ length: count }, (_, n) => settle(`benchmark-${run}-${n}`, world, [ticks])));
    walls.push(performance.now() - started);
    run += 1;
    digests = new Set(results.map((result) => digest(result.get(ticks))));
  }
  const wall = [...walls].sort((a, b) => a - b)[Math.floor(walls.length / 2)];
  console.log(JSON.stringify({ title: conversation.title, blocks: world.size(), ticks, count, engines: poolSize(), wallMs: Math.round(wall), settlesPerSecond: Math.round(count / wall * 10000) / 10, digests: [...digests] }));
  await stopShotEngines();
  store.close();
}

main().catch((error) => { console.error(error); process.exit(1); });
