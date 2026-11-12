#!/usr/bin/env node
'use strict';

const crypto = require('crypto');
const { performance } = require('perf_hooks');

const { MAX_BLOCKS, World } = require('../server/world');

function elapsed(start) {
  return Math.round((performance.now() - start) * 10) / 10;
}

const world = new World();
let start = performance.now();
world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
const fillMs = elapsed(start);
world.finalize();

start = performance.now();
const candidate = world.clone();
candidate.fill([120, 80, 20], [179, 119, 29], 'minecraft:oak_planks', {});
const diff = candidate.diff(world);
const editMs = elapsed(start);

let persistedBytes = 0;
start = performance.now();
const manifest = candidate.serializeManifest((blocks) => {
  const payload = JSON.stringify(blocks);
  persistedBytes += Buffer.byteLength(payload);
  return crypto.createHash('sha256').update(payload).digest('hex').slice(0, 24);
});
const manifestMs = elapsed(start);

start = performance.now();
let renderBlocks = 0;
for (const key of candidate.chunks.keys()) {
  renderBlocks += candidate.chunkSnapshot(key, true, true).blocks.length;
}
const renderSnapshotMs = elapsed(start);

if (global.gc) global.gc();
const heapMiB = Math.round(process.memoryUsage().heapUsed / 1048576);
const result = {
  blocks: world.size(),
  chunks: world.chunks.size,
  fillMs,
  localizedEditMs: editMs,
  diff,
  manifestMs,
  chunkPayloadMiB: Math.round(persistedBytes / 1048576),
  manifestChunks: manifest.chunks.length,
  renderBlocks,
  renderSnapshotMs,
  heapMiB,
};
process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);

if (world.size() !== MAX_BLOCKS || fillMs > 2000 || editMs > 500
  || renderBlocks > 250000 || renderSnapshotMs > 2000 || heapMiB > 512) {
  process.exitCode = 1;
}
