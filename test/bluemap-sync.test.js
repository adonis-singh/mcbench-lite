'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// A small native-tile stand-in records the visible scene and resource
// lifetime, and a mesher stand-in holds one world as the worker does: tile x
// (z = 0) of a world drawn as `values[x]`. The synchronizer still uses its
// real loading, replacement and reveal path.
async function fixture(t) {
  const { BlueMapTileSync } = await import('../web/bluemap-tiles.mjs');
  const scene = new Set();
  const allocated = [];
  class Tile {
    constructor(x, z, onLoad, onUnload) {
      Object.assign(this, { x, z, onLoad, onUnload, model: null, loading: false });
      allocated.push(this);
    }
    get loaded() { return this.model !== null; }
    async load(loader) {
      this.loading = true;
      try {
        this.model = await loader.load(this.x, this.z);
        this.onLoad(this);
      } catch { this.unload(); }
      finally { this.loading = false; }
    }
    unload() {
      if (this.model) this.onUnload(this);
      this.model = null;
    }
  }
  const loader = {
    fileLoader: {},
    bufferGeometryLoader: { parse() { throw new Error('the stand-in tiles carry no geometry'); } },
    // BlueMap's address for a tile, with its digits split into folders.
    load(x, z) {
      const digits = (value) => `${value < 0 ? '-' : ''}${String(Math.abs(value)).split('').join('/')}`;
      return new Promise((resolve, reject) => this.fileLoader.load(`/bluemap/abc/maps/live/tiles/0/x${digits(x)}/z${digits(z)}.prbm`, async (data) => {
        const value = new TextDecoder().decode(data);
        if (value === 'slow') await new Promise((r) => setTimeout(r, 20));
        if (value === 'broken') reject(new Error('broken geometry'));
        else resolve(value);
      }, null, reject));
    },
  };
  const manager = {
    tiles: new Map(), tileLoader: loader, centerTile: { x: 0, y: 0 }, viewDistanceX: 4, viewDistanceZ: 4,
    removeAllTiles() { for (const tile of this.tiles.values()) tile.unload(); this.tiles.clear(); },
    handleLoadedTile(tile) { scene.add(tile); },
    handleUnloadedTile(tile) { scene.delete(tile); },
    tryLoadTile(x, z) {
      const tile = new Tile(x, z, this.handleLoadedTile, this.handleUnloadedTile);
      this.tiles.set(`x${x}z${z}`, tile);
      tile.load(loader);
      return true;
    },
  };
  const lowres = { removed: 0, removeAllTiles() { this.removed += 1; } };
  const viewer = {
    map: { hiresTileManager: manager, lowresTileManager: [lowres] },
    updateLoadedMapArea() { while (manager.loadNextTile()) {} },
  };
  const views = new Map(); // blueprint address -> values
  const draws = [];
  const loads = [];
  let world = null;
  const mesher = {
    async load(view, compare) {
      loads.push({ view, compare });
      const values = views.get(view.blueprint);
      if (!values) throw new Error(`${view.blueprint} is unavailable (HTTP 404)`);
      const before = world;
      world = values;
      const tiles = values.flatMap((value, x) => (value === null ? [] : [[x, 0]]));
      return { tiles, changed: tiles.filter(([x]) => !compare || !before || before[x] !== values[x]) };
    },
    // A patch here is JSON: the world's values after it and the tiles it changed.
    async patch(bytes) {
      const { values, changed } = JSON.parse(new TextDecoder().decode(bytes));
      world = values;
      return { tiles: values.flatMap((value, x) => (value === null ? [] : [[x, 0]])), changed };
    },
    async draw(x, z) {
      draws.push(`${x},${z}`);
      return new TextEncoder().encode(world[x]).buffer;
    },
  };
  Object.defineProperty(globalThis, 'requestAnimationFrame', { configurable: true, value: (callback) => setImmediate(callback) });
  t.after(() => delete globalThis.requestAnimationFrame);
  const sync = new BlueMapTileSync({ mapViewer: viewer }, mesher);
  let published = 0;
  const publish = (...values) => {
    const blueprint = `/api/minecraft/blueprint/${published += 1}`;
    views.set(blueprint, values);
    return { blueprint, mask: { minX: 0, maxX: 127, minZ: 0, maxZ: 127 }, wasm: '/bluemap/mesher.wasm', pack: '/bluemap/abc/mcbench-pack.bin' };
  };
  return { sync, manager, lowres, scene, allocated, publish, draws, loads };
}

test('unchanged geometry survives refresh; changed, deleted, and late tiles converge', async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('first', 'second'));
  const first = f.manager.tiles.get('x0z0');
  const second = f.manager.tiles.get('x1z0');
  await f.sync.refresh(f.publish('first', 'second'));
  assert.equal(f.manager.tiles.get('x0z0'), first);
  assert.equal(f.manager.tiles.get('x1z0'), second);
  assert.equal(f.allocated.length, 2);
  assert.deepEqual(f.draws, ['0,0', '1,0'], 'unchanged geometry is not drawn again');
  await f.sync.refresh(f.publish('changed', null));
  assert.equal(first.loaded, false);
  assert.equal(second.loaded, false);
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['changed']);
  const retained = f.manager.tiles.get('x0z0');
  await f.sync.refresh(f.publish('changed', 'late'));
  assert.equal(f.manager.tiles.get('x0z0'), retained);
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['changed', 'late']);
  assert.deepEqual(f.loads.map((load) => load.compare), [true, true, true, true], 'edits of one chat are compared with the world before');
  assert.equal(f.lowres.removed, 1);
  assert.equal(f.lowres.loadNextTile(), false, 'no lowres tile is ever loaded');
});

test('live patches draw the tiles they change or add, drop the tiles the world no longer has, and swap nothing for a patch that changes nothing', async (t) => {
  const f = await fixture(t);
  const patch = (values, changed) => new TextEncoder().encode(JSON.stringify({ values, changed }));
  await f.sync.refresh(f.publish('a', 'b'));
  const kept = f.manager.tiles.get('x0z0');
  f.draws.length = 0;
  // The world grows a tile; tile 1 changes.
  await f.sync.patch([patch(['a', 'b1', 'c'], [[1, 0]]), patch(['a', 'b2', 'c'], [[1, 0], [2, 0]])]);
  assert.deepEqual(f.draws.sort(), ['1,0', '2,0']);
  assert.equal(f.manager.tiles.get('x0z0'), kept, 'a tile no patch changed stays');
  assert.deepEqual([...f.scene].map((tile) => tile.model).sort(), ['a', 'b2', 'c']);
  // It shrinks back: the tiles it no longer has go, nothing is drawn.
  f.draws.length = 0;
  await f.sync.patch([patch(['a', null, null], [])]);
  assert.deepEqual(f.draws, []);
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['a']);
  assert.deepEqual([...f.manager.tiles.keys()], ['x0z0']);
  await f.sync.patch([patch(['a', null, null], [])]);
  assert.deepEqual(f.draws, [], 'a patch that changes nothing draws nothing');
});

test('a refresh or patch asked during another is refused: they run one at a time', async (t) => {
  const f = await fixture(t);
  const patch = new TextEncoder().encode(JSON.stringify({ values: ['a'], changed: [[0, 0]] }));
  const first = f.sync.refresh(f.publish('slow', 'b'));
  await assert.rejects(f.sync.patch([patch]), /already running/);
  await assert.rejects(f.sync.refresh(f.publish('c')), /already running/);
  await first;
  await f.sync.patch([patch]);
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['a'], 'the one after runs');
});

test('a failed replacement retains the visible scene and disposes every prepared tile', async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('first', 'second'));
  const before = [...f.scene];
  await assert.rejects(f.sync.refresh(f.publish('broken', 'slow')), /could not be decoded/);
  assert.deepEqual([...f.scene], before);
  assert.ok(before.every((tile) => tile.loaded));
  assert.ok(f.allocated.slice(2).every((tile) => !tile.loaded && !tile.loading));
  await f.sync.refresh(f.publish('fixed', 'slow'));
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['fixed', 'slow']);
});

test('a view whose blueprint is gone fails without touching the scene', async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('first'));
  const before = [...f.scene];
  await assert.rejects(f.sync.refresh({ ...f.publish('second'), blueprint: '/api/minecraft/blueprint/999' }), /HTTP 404/);
  assert.deepEqual([...f.scene], before);
  await f.sync.refresh(f.publish('second'));
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['second'], 'the next view is compared with the world still loaded');
});

test('moving the camera during drawing does not restore tiles that left its view', async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('first'));
  const refresh = f.sync.refresh(f.publish('slow'));
  await new Promise((resolve) => setTimeout(resolve, 5));
  f.manager.centerTile.x = 100;
  f.manager.removeAllTiles();
  await refresh;
  assert.equal(f.manager.tiles.size, 0);
  assert.equal(f.scene.size, 0);
  assert.ok(f.allocated.every((tile) => !tile.loaded && !tile.loading));
});

test('a populated chat clears when the selected chat has an empty view', async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('old build'));
  await f.sync.refresh(f.publish(), { fresh: true });
  assert.equal(f.scene.size, 0);
  assert.equal(f.manager.tiles.size, 0);
  assert.equal(f.sync.assets.size, 0);
  assert.ok(f.allocated.every((tile) => !tile.loaded));
});

test('a large map is revealed after its first hires tile instead of waiting for every tile', async t => {
  const f = await fixture(t);
  const load = f.manager.tileLoader.load.bind(f.manager.tileLoader);
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  f.manager.tileLoader.load = async (x, z) => {
    if (x === 1) await gate;
    return load(x, z);
  };
  let timer;
  try {
    await Promise.race([f.sync.refresh(f.publish('first', 'later')), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Viewer waited for all geometry')), 500);
    })]);
    assert.equal(f.manager.tiles.get('x0z0').loaded, true);
    assert.equal(f.manager.tiles.get('x1z0').loading, true);
    release();
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(f.manager.tiles.get('x1z0').loaded, true);
  } finally { clearTimeout(timer); release(); }
});

test("another chat's world streams in fresh: the previous build is cleared at once", async (t) => {
  const f = await fixture(t);
  await f.sync.refresh(f.publish('old build', 'old wing'));
  const old = [...f.scene];
  const refresh = f.sync.refresh(f.publish('new build'), { fresh: true });
  assert.ok(old.every((tile) => !tile.loaded), 'old tiles leave before the new world is loaded');
  await refresh;
  assert.deepEqual([...f.scene].map((tile) => tile.model), ['new build']);
  assert.equal(f.loads.at(-1).compare, false, 'another chat is drawn whole');
});

test('indexed tiles take their material groups from after the indices', async () => {
  const { BlueMapTileSync } = await import('../web/bluemap-tiles.mjs');
  // BlueMap's loader reads groups from where the indices begin; the bridge rereads them.
  const data = new ArrayBuffer(64);
  const view = new DataView(data);
  const index = new Uint32Array(data, 16, 6); index.set([0, 1, 2, 0, 2, 3]);
  [[3, 0, 6], [-1]].flat().forEach((value, i) => view.setInt32(40 + i * 4, value, true));
  const geometry = { index: { array: index }, groups: [{ materialIndex: 0, start: 1, count: 2 }] };
  const loader = { fileLoader: {}, bufferGeometryLoader: { parse: () => geometry }, textureLoader: {} };
  const manager = { tiles: new Map(), tileLoader: loader, removeAllTiles() {} };
  new BlueMapTileSync({ mapViewer: { map: { hiresTileManager: manager, lowresTileManager: [] } } }, null);
  assert.deepEqual(loader.bufferGeometryLoader.parse(data).groups, [{ materialIndex: 3, start: 0, count: 6 }]);
});
