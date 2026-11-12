'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { pathToFileURL } = require('url');

const { build } = require('../scripts/build-wasm');
const { LiveViews } = require('../server/live-views');
const { World } = require('../server/world');
const { startNodeEngine } = require('../scripts/wasm-node-engine');

// The page's live world (web/live-world.mjs) against the real views and the
// real engine in a worker.
let wasm;
let LiveWorld;
test.before(async () => {
  wasm = await build();
  ({ LiveWorld } = await import('../web/live-world.mjs'));
});

function build3(...blocks) {
  const world = new World();
  for (const [x, y, z, id, states] of blocks) world.set(x, y, z, id, states || {});
  world.finalize();
  return world;
}

// A server over the real views.
function server(resolve) {
  return { views: new LiveViews({ resolve, yOffset: 64, now: () => Date.now() * 1000 }) };
}

function host(t, { views }, { wasmUrl = pathToFileURL(wasm).href } = {}) {
  const urls = new Map();
  const calls = [];
  const engines = [];
  const route = async (path_, { method = 'GET' } = {}) => {
    calls.push([method, path_]);
    const url = new URL(path_, 'http://app');
    if (url.pathname === '/api/minecraft/status') {
      const key = url.searchParams.get('view');
      return { minecraftVersion: '1.21.10', yOffset: 64, viewerUrl: '/bluemap/', mesher: { wasm: '/m.wasm', pack: '/m.pack' }, viewerError: null, core: wasmUrl, coreError: null, view: key ? views.describe(key) : null };
    }
    if (url.pathname === '/api/minecraft/import') return views.imports(url.searchParams.get('view'), url.searchParams.has('since') ? Number(url.searchParams.get('since')) : null);
    throw new Error(`no route ${path_}`);
  };
  return {
    calls,
    engines,
    json: async (path_, options) => JSON.parse(JSON.stringify(await route(path_, options))),
    startEngine: async (url) => { const engine = await startNodeEngine(fileURLToPath(url)); engines.push(engine); t.after(() => engine.terminate()); return engine; },
    blobUrl: (bytes) => { const url = `blob:${urls.size}`; urls.set(url, bytes); return url; },
    revoke: (url) => urls.delete(url),
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, Math.min(ms, 20))),
    urls,
  };
}

const { fileURLToPath } = require('url');

async function until(check, what, ms = 8000) {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = check();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function open(t, resolve, options) {
  const h = host(t, server(resolve), options);
  const live = new LiveWorld(h);
  t.after(() => live.dispose());
  live.begin();
  return { live, h };
}

test('the build on show is placed in the page\'s engine, a base for the viewer, and ticks on its own', async (t) => {
  const world = build3([0, 0, 0, 'minecraft:stone'], [1, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' }]);
  const { live, h } = open(t, () => ({ world, layout: null }));
  const frames = [];
  live.onFrame((frame) => frames.push(frame));
  await live.show('chat');
  await until(() => live.state.viewerRevision, 'the base');
  assert.strictEqual(live.state.status, 'running');
  assert.strictEqual(live.state.conversationId, 'chat');
  assert.strictEqual(live.state.blockCount, 2);
  assert.ok(h.urls.has(live.state.viewerBlueprint));
  await until(() => frames.length >= 4, 'four ticks');
  assert.ok(frames.every((frame) => frame.base === live.state.viewerRevision));
  assert.ok(live.ticks >= 4);
  // A viewer that came late is given what happened since its base in one frame.
  assert.ok(live.accumulated().patch.length > 0);
});

test('a click on a lever is the engine\'s input, and on other blocks nothing', async (t) => {
  const world = build3([0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' }], [1, 1, 0, 'minecraft:redstone_lamp']);
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.state.viewerRevision, 'the base');
  assert.strictEqual(await live.input(0, 64, 0, 0), false, 'stone takes no input');
  assert.strictEqual(await live.input(0, 65, 0, 0), true);
  await new Promise((resolve) => setTimeout(resolve, 150));
  // The lever's power reached the engine: the next patches changed its block.
  const patch = live.accumulated();
  assert.ok(patch);
  assert.strictEqual(live.inputs.at(0, 65, 0), 'toggle');
});

test('an edit within the drawn area reaches the viewer as a patch on its base, and the count goes on', async (t) => {
  const { readPatch } = await import('../web/live-patch.mjs');
  let world = build3([0, 0, 0, 'minecraft:stone']);
  const { live, h } = open(t, () => ({ world, layout: null }));
  const frames = [];
  live.onFrame((frame) => frames.push(frame));
  await live.show('chat');
  const base = await until(() => live.state.viewerRevision, 'the base');
  await until(() => live.ticks >= 3, 'ticks');
  const before = live.ticks;
  world = build3([0, 0, 0, 'minecraft:stone'], [1, 0, 0, 'minecraft:oak_planks']);
  await until(() => live.state.blockCount === 2, 'the edit');
  assert.strictEqual(live.state.viewerRevision, base, 'the viewer keeps its base');
  assert.ok(live.ticks >= before, 'an edit does not restart the world');
  const planks = frames.find((frame) => readPatch(frame.patch).changes.some(([x, y, z]) => x === 1 && y === 64 && z === 0));
  assert.ok(planks, 'a frame carries the edit');
  assert.strictEqual(planks.base, base);
  assert.ok(h.calls.some(([, call]) => /import\?view=chat&since=/.test(call)));
});

test('an edit past the drawn area is placed on the world as it is, as a new base', async (t) => {
  let world = build3([0, 0, 0, 'minecraft:stone']);
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  const base = await until(() => live.state.viewerRevision, 'the base');
  await until(() => live.ticks >= 3, 'ticks');
  const before = live.ticks;
  world = build3([0, 0, 0, 'minecraft:stone'], [400, 0, 0, 'minecraft:oak_planks']);
  await until(() => live.state.viewerRevision > base, 'the new base');
  assert.ok(live.ticks >= before, 'an edit does not restart the world');
  assert.strictEqual(live.state.blockCount, 2);
});

test('another build is placed afresh and its ticks counted again', async (t) => {
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone']), b: build3([5, 0, 5, 'minecraft:glass']) };
  const { live } = open(t, (key) => ({ world: worlds[key], layout: null }));
  await live.show('a');
  await until(() => live.ticks >= 5, 'ticks on a');
  await live.show('b');
  await until(() => live.state.conversationId === 'b' && live.state.viewerRevision && live.ticks < 5, 'b placed afresh');
  assert.strictEqual(live.state.blockCount, 1);
});

test('an edit that reaches what the engine does not model stops the world with the reason; a restart places it afresh', async (t) => {
  let world = build3([0, 0, 0, 'minecraft:stone']);
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.ticks >= 2, 'ticks');
  world = build3([0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:resin_clump', { down: 'true' }]);
  await until(() => live.state.status === 'error', 'the refusal');
  assert.match(live.state.error, /^The engine failed: unported: /);
  world = build3([0, 0, 0, 'minecraft:stone']);
  await new Promise((resolve) => setTimeout(resolve, 100));
  await live.control('restart');
  await until(() => live.state.status === 'running' && live.ticks >= 1, `the world placed afresh (${live.state.error})`);
});

test('a build the engine cannot place shows the reason in its place', async (t) => {
  const world = build3([0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:resin_clump', { down: 'true' }]);
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.state.status === 'error', 'the refusal');
  assert.match(live.state.error, /^The engine failed: unported: /);
  assert.strictEqual(live.state.viewerBlueprint, null);
});

test('the world exports as a Minecraft save', async (t) => {
  const world = build3([0, 0, 0, 'minecraft:stone'], [20, 0, 20, 'minecraft:stone']);
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.ticks >= 1, 'a tick');
  const { folder, bytes } = await live.exportWorld('My build');
  assert.strictEqual(folder, 'My build');
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-export-')), 'world.zip');
  fs.writeFileSync(file, bytes);
  const { readZipEntry } = require('../server/zip-entry');
  assert.ok(readZipEntry(file, 'My build/level.dat').length > 100);
  assert.ok(readZipEntry(file, 'My build/region/r.0.0.mca').length >= 8192);
});

test('a build with no blocks shows an empty world with no engine, and the first block is placed afresh', async (t) => {
  let world = new World();
  world.finalize();
  const { live, h } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.state.viewerRevision, 'the empty base');
  assert.strictEqual(live.state.status, 'running');
  assert.strictEqual(live.state.live, null, 'there is no world to play');
  assert.strictEqual(h.engines.length, 0, 'no engine was started');
  assert.strictEqual(h.urls.size, 1);
  world = build3([0, 0, 0, 'minecraft:stone']);
  await until(() => live.state.live, 'the world of the first block');
  assert.strictEqual(live.state.blockCount, 1);
  assert.ok(h.calls.filter(([, call]) => call.startsWith('/api/minecraft/import')).every(([, call]) => !call.includes('since')), 'placed afresh: the page held no world');
});
