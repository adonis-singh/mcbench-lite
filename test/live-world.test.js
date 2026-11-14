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

// The host's kept placements (live-host.mjs keeps them in IndexedDB), in memory.
function memoryPlacements() {
  const kept = new Map();
  return {
    kept,
    get: async (key) => kept.get(key) ?? null,
    put: async (key, entry) => { kept.set(key, entry); },
  };
}
const digest = async (text) => require('node:crypto').createHash('sha256').update(text).digest('hex');

function host(t, { views }, { wasmUrl = pathToFileURL(wasm).href, placements = memoryPlacements() } = {}) {
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
    digest,
    placements,
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
  // The base is on show while its sync is still ending (the engine is asked
  // to run, the placement is kept): the status is 'running' once it has.
  await until(() => live.state.viewerRevision && live.state.status === 'running', 'the base, its sync ended');
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

test('the page\'s index of what a click means is the engine\'s, for the base and for the blocks a patch brings', async (t) => {
  const wire = { north: 'none', east: 'none', south: 'none', west: 'none', power: '0' };
  const door = { facing: 'south', half: 'lower', hinge: 'left', open: 'false', powered: 'false' };
  const world = build3(
    [0, 0, 0, 'minecraft:stone'], [2, 0, 0, 'minecraft:stone'], [4, 0, 0, 'minecraft:stone'], [6, 0, 0, 'minecraft:stone'],
    [0, 1, 0, 'minecraft:oak_door', door], [0, 2, 0, 'minecraft:oak_door', { ...door, half: 'upper' }],
    [2, 1, 0, 'minecraft:iron_door', door], [2, 2, 0, 'minecraft:iron_door', { ...door, half: 'upper' }],
    [4, 1, 0, 'minecraft:redstone_wire', wire],
    [6, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' }],
  );
  const { live } = open(t, () => ({ world, layout: null }));
  await live.show('chat');
  await until(() => live.state.viewerRevision && live.state.status === 'running', 'the base');
  assert.strictEqual(live.inputs.at(0, 65, 0), 'use');
  assert.strictEqual(live.inputs.at(2, 65, 0), null, 'iron is not opened by hand');
  assert.strictEqual(live.inputs.at(4, 65, 0), 'use', 'a dot of dust');
  assert.strictEqual(await live.input(2, 65, 0, 0), false, 'nothing is sent for the iron door');
  assert.strictEqual(await live.input(0, 65, 0, 0), true);
  assert.strictEqual(await live.input(4, 65, 0, 0), true);
  assert.strictEqual(await live.input(6, 65, 0, 0), true);
  // The patches bring the open door, the cross and the pulled lever, each a palette entry the engine names the input of.
  await until(() => live.inputs.kinds.length > 6, 'the patch\'s palette entries');
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.strictEqual(live.inputs.at(0, 65, 0), 'use');
  assert.strictEqual(live.inputs.at(4, 65, 0), 'use', 'a cross');
  assert.strictEqual(live.inputs.at(6, 65, 0), 'toggle');
  assert.strictEqual(await live.input(4, 65, 0, 0), true, 'and a lone cross becomes a dot');
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

test('a restart keeps the player standing where it was, its look, flight and keys, and the count starts again', async (t) => {
  const world = build3(...Array.from({ length: 25 }, (_, i) => [i % 5, 0, Math.floor(i / 5), 'minecraft:stone']));
  const { live } = open(t, () => ({ world, layout: null }));
  const views = [];
  live.onFrame(({ view, ticks }) => { if (view) views.push({ view: Array.from(view), ticks }); });
  let gone = 0;
  live.onPlayerGone(() => { gone += 1; });
  await live.show('chat');
  await until(() => live.ticks >= 2, 'ticks');
  await live.spawnPlayer({ x: 2.5, y: 70, z: 2.5, yaw: 90, pitch: 20, flying: true });
  // Forward, flying: it moves along -x at yaw 90.
  await live.setPlayerInput({ keys: 1, taps: 0, yaw: 90, pitch: 20 });
  await until(() => views.length >= 10, 'the player moving');
  await live.control('pause');
  const before = Array.from(live.playerView);
  assert.ok(before[0] < 2.5, `it moved (${before[0]})`);
  await live.control('restart');
  await until(() => live.ticks === 0 && live.phase === 'waiting' && !live.restarting, 'placed afresh');
  assert.strictEqual(gone, 0, 'the viewer is not told the player is gone');
  assert.strictEqual(live.player, 'standing');
  const after = Array.from(live.playerView);
  assert.deepStrictEqual(after.slice(0, 3), before.slice(0, 3), 'the same position');
  assert.deepStrictEqual([after[9], after[10], after[17]], [before[9], before[10], 1], 'the same look, flying');
  // The keys it held still move it: the engine holds them after the placement.
  views.length = 0;
  await live.control('play');
  await until(() => views.length >= 5, 'ticks after the restart');
  assert.ok(views[0].ticks <= 2, 'the count started again');
  assert.ok(views.at(-1).view[0] < before[0], `still moving forward (${views.at(-1).view[0]} from ${before[0]})`);
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

test('a build shown again at the same version is drawn from its last placement while the engine places it again, and takes no input until then', async (t) => {
  const lever = [0, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' }];
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone'], lever), b: build3([5, 0, 5, 'minecraft:dirt']) };
  const { live, h } = open(t, (key) => ({ world: worlds[key], layout: null }));
  // Once `gate` is set, a placement waits until the test lets it go.
  let gate = null;
  let placing = false;
  let placed = false;
  const start = h.startEngine;
  h.startEngine = async (url) => {
    const engine = await start(url);
    const place = engine.place.bind(engine);
    engine.place = async (...args) => {
      if (!gate) return place(...args);
      placing = true;
      await gate;
      const blueprint = await place(...args);
      placed = true;
      return blueprint;
    };
    return engine;
  };
  await live.show('a');
  await until(() => live.state.status === 'running' && live.state.live, 'a placed');
  const first = h.urls.get(live.state.viewerBlueprint);
  await live.show('b');
  await until(() => live.state.status === 'running' && live.state.conversationId === 'b' && live.state.live, 'b placed');
  let release;
  gate = new Promise((resolve) => { release = resolve; });
  await live.show('a');
  await until(() => live.state.conversationId === 'a' && live.state.viewerRevision != null && placing, 'a shown before its placement');
  assert.strictEqual(live.state.status, 'syncing');
  assert.strictEqual(live.state.live, null, 'no controls while the base is not placed');
  assert.deepStrictEqual(h.urls.get(live.state.viewerBlueprint), first, 'the base shown is the last placement');
  assert.strictEqual(live.accumulated(), null);
  assert.strictEqual(await live.input(0, 65, 0, 0), false, 'the lever takes no click yet');
  await assert.rejects(live.control('step'), /no live world on show/);
  // A spawn asked now waits for the placement: player requests run in turn.
  const spawned = live.spawnPlayer({ x: 0, y: 70, z: 0, yaw: 0, pitch: 0, flying: true }).then(() => placed && !live.base.provisional);
  const revision = live.state.viewerRevision;
  release();
  assert.strictEqual(await spawned, true, 'the player stood in the world only once it was placed and confirmed');
  await until(() => live.state.status === 'running' && live.state.live, 'a placed again');
  assert.strictEqual(live.state.viewerRevision, revision, 'the placement confirmed the base shown: no second base');
  assert.strictEqual(live.state.live.ticks, live.ticks);
  assert.strictEqual(await live.input(0, 65, 0, 0), true, 'and then the lever takes it');
});

test('a placement that differs from the base shown while it was placed fails the sync, said where the build is', async (t) => {
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone']), b: build3([5, 0, 5, 'minecraft:dirt']) };
  const { live, h } = open(t, (key) => ({ world: worlds[key], layout: null }));
  await live.show('a');
  await until(() => live.state.status === 'running' && live.state.live && h.placements.kept.has('a'), 'a placed and kept');
  await live.show('b');
  await until(() => live.state.status === 'running' && live.state.conversationId === 'b', 'b placed');
  const kept = h.placements.kept.get('a');
  const bytes = kept.bytes.slice();
  bytes[bytes.length - 1] ^= 1;
  kept.bytes = bytes;
  await live.show('a');
  await until(() => live.state.status === 'error', 'the sync failed');
  assert.match(live.state.error, /differs from the one shown/);
  assert.strictEqual(live.state.live, null);
});

test('a view left goes with its engine at once: the next view does not wait for its placement', async (t) => {
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone']), b: build3([5, 0, 5, 'minecraft:dirt']) };
  const { live, h } = open(t, (key) => ({ world: worlds[key], layout: null }));
  // a's placement never ends on its own: only the end of its engine ends it,
  // as it ends every request of that engine.
  const start = h.startEngine;
  h.startEngine = async (url) => {
    const engine = await start(url);
    const place = engine.place.bind(engine);
    const terminate = engine.terminate.bind(engine);
    let stop;
    engine.place = (...args) => (live.key === 'a' ? new Promise((resolve, reject) => { stop = reject; }) : place(...args));
    engine.terminate = (error) => { terminate(error); stop?.(new Error('The engine was stopped')); };
    return engine;
  };
  await live.show('a');
  await until(() => live.phase === 'syncing' && live.engine, 'a placing');
  const left = live.engine;
  await live.show('b');
  await until(() => live.state.status === 'running' && live.state.conversationId === 'b' && live.state.live, 'b placed while a never was');
  assert.ok(left.dead, "a's engine was ended");
  assert.notStrictEqual(live.engine, left);
});

test('a placement kept by an earlier page is drawn at once by the next one, and confirmed by its own engine before anything ticks', async (t) => {
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' }]) };
  const resolve = (key) => ({ world: worlds[key], layout: null });
  const views = server(resolve);
  const placements = memoryPlacements();
  // The first page places the view and keeps its placement.
  const before = host(t, views, { placements });
  const first = new LiveWorld(before);
  first.begin();
  await first.show('a');
  await until(() => first.state.status === 'running' && placements.kept.has('a'), 'a placed and kept');
  const shown = before.urls.get(first.state.viewerBlueprint);
  first.dispose();
  // A page loaded later, with the same server and the same kept placements.
  const after = host(t, views, { placements });
  let release;
  const gate = new Promise((done) => { release = done; });
  let placing = false;
  const start = after.startEngine;
  after.startEngine = async (url) => {
    const engine = await start(url);
    const place = engine.place.bind(engine);
    engine.place = async (...args) => { placing = true; await gate; return place(...args); };
    return engine;
  };
  const live = new LiveWorld(after);
  t.after(() => live.dispose());
  live.begin();
  await live.show('a');
  await until(() => live.state.viewerRevision != null && placing, 'a shown before its placement');
  assert.deepStrictEqual(after.urls.get(live.state.viewerBlueprint), shown, 'the kept placement is the base shown');
  assert.strictEqual(live.state.live, null);
  assert.strictEqual(await live.input(0, 65, 0, 0), false, 'the lever takes no click yet');
  release();
  await until(() => live.state.status === 'running' && live.state.live, 'placed and confirmed');
  assert.strictEqual(await live.input(0, 65, 0, 0), true);
});

test('a kept placement is chosen by its commands, not by version: one of other commands is not shown', async (t) => {
  const worlds = { a: build3([0, 0, 0, 'minecraft:stone']) };
  const placements = memoryPlacements();
  const h = host(t, server((key) => ({ world: worlds[key], layout: null })), { placements });
  // What a server before a restart left for this view: other blocks.
  await placements.put('a', { core: pathToFileURL(wasm).href, digest: await digest('setblock 9 64 9 minecraft:dirt\n'), bytes: new Uint8Array([1, 2, 3]) });
  const bases = [];
  const blobUrl = h.blobUrl;
  h.blobUrl = (bytes) => { bases.push(bytes); return blobUrl(bytes); };
  const live = new LiveWorld(h);
  t.after(() => live.dispose());
  live.begin();
  await live.show('a');
  await until(() => live.state.status === 'running' && live.state.live, 'a placed');
  assert.strictEqual(bases.length, 1, 'one base: the engine\'s own');
  assert.notDeepStrictEqual(bases[0], new Uint8Array([1, 2, 3]));
  assert.notDeepStrictEqual(placements.kept.get('a').bytes, new Uint8Array([1, 2, 3]), 'and it is kept in place of the stale one');
});
