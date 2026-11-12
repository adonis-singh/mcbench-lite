'use strict';

const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { test } = require('node:test');

const { MinecraftLive } = require('../server/minecraft-live');
const { World } = require('../server/world');

function host(options) {
  const paperDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-live-test-'));
  const blueMap = {
    assets: null,
    async prepare() { this.assets = { id: 'test', viewerUrl: '/bluemap/test/', pack: '/bluemap/test/mcbench-pack.bin', wasm: '/bluemap/mesher-test.wasm' }; },
  };
  return new MinecraftLive({ paperDir, blueMap, ...options });
}

test('the server tells the page what it needs to draw a world, as soon as the viewer is prepared', async () => {
  const live = host({ blockTable: async () => 'table.tsv' });
  assert.deepStrictEqual(live.status(), { minecraftVersion: '1.21.10', yOffset: 64, viewerUrl: null, mesher: null, viewerError: null });
  await live.prepareViewer();
  assert.deepStrictEqual(live.status(), {
    minecraftVersion: '1.21.10', yOffset: 64, viewerUrl: '/bluemap/test/', mesher: { wasm: '/bluemap/mesher-test.wasm', pack: '/bluemap/test/mcbench-pack.bin' }, viewerError: null,
  });
});

test('a viewer that cannot be prepared says so, and is prepared again when the page asks, and not back to back', async (t) => {
  t.mock.timers.enable({ apis: ['Date'] });
  let attempts = 0;
  let java = false;
  const live = host({ blockTable: async () => { attempts += 1; if (!java) throw new Error('no Java'); return 'table.tsv'; } });
  const pause = () => new Promise((resolve) => setTimeout(resolve, 10));
  const logged = [];
  const quiet = console.error;
  console.error = (...args) => logged.push(args.join(' '));
  try {
    // The server's start prepares once, and nothing else calls prepareViewer.
    live.warm();
    await pause();
    assert.deepStrictEqual(logged, ['[viewer] no Java']);
    // The page asks every second. It is told of the failure, and nothing is
    // tried that soon after it.
    assert.strictEqual(live.status().viewerError, 'The viewer could not be prepared: no Java');
    assert.strictEqual(attempts, 1);
    // Half a minute after the failure, its asking tries again.
    t.mock.timers.tick(30_000);
    assert.strictEqual(live.status().viewerError, 'The viewer could not be prepared: no Java');
    assert.strictEqual(attempts, 2);
    assert.strictEqual(live.status().viewerError, null, 'preparing again');
    await pause();
    java = true;
    t.mock.timers.tick(30_000);
    assert.strictEqual(live.status().viewerError, 'The viewer could not be prepared: no Java');
    await pause();
    assert.deepStrictEqual(logged, ['[viewer] no Java'], 'the tries the page asked for are told to the page');
  } finally {
    console.error = quiet;
  }
  const status = live.status();
  assert.strictEqual(status.viewerError, null);
  assert.strictEqual(status.viewerUrl, '/bluemap/test/');
  assert.strictEqual(attempts, 3, 'ready assets are not prepared again');
});

test('the viewer is prepared once while that runs, and once it is ready', async () => {
  let attempts = 0;
  let release;
  const live = host({ blockTable: () => { attempts += 1; return new Promise((resolve) => { release = () => resolve('table.tsv'); }); } });
  const first = live.prepareViewer();
  assert.strictEqual(live.prepareViewer(), first);
  assert.strictEqual(live.viewer.name, 'preparing');
  release();
  await first;
  assert.strictEqual(live.viewer.name, 'ready');
  assert.strictEqual(live.prepareViewer(), first);
  assert.strictEqual(attempts, 1);
});

test('a build is placed afresh by the server\'s engine, one at a time', async () => {
  const placed = [];
  const engine = {
    async warm() {},
    async place(commands, blueprint, { fresh }) { placed.push([fs.readFileSync(commands, 'utf8'), fresh]); fs.writeFileSync(blueprint, `blueprint ${placed.length}`); },
    async stop() {},
  };
  const live = host({ engine, ensurePaper: undefined });
  live.ensurePaper = async () => [];
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  world.finalize();
  const [first, second] = await Promise.all([live.drawBlueprint(world), live.placeBlueprint('chat', world)]);
  assert.deepStrictEqual([first.toString(), second.toString()], ['blueprint 1', 'blueprint 2']);
  assert.deepStrictEqual(placed, [['setblock 0 64 0 minecraft:stone\n', true], ['setblock 0 64 0 minecraft:stone\n', true]]);
});
