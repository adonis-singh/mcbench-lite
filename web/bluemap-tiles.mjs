const hashTile = (x, z) => `x${x}z${z}`;

// BlueMap's address for a hires tile: x and z with their digits split into
// folders ("tiles/0/x-1/2/z3/4.prbm" is tile -12, 34).
const TILE = /\/tiles\/0\/x(-?[\d/]+)\/z(-?[\d/]+)\.prbm$/;

// The viewer's mesher workers (mesher-worker.mjs), `size` of them: one holds
// the world (it loads views, takes patches and cuts each tile's snapshot,
// what the tile reads of the world) and the others draw tiles from
// snapshots, so a tab holds its world once however many draw. Requests keep the order they were
// asked in: a draw's snapshot is cut after every load and patch asked before
// it and before any asked after it, and only when a drawing worker is free to
// take it, so waiting draws hold no snapshot.
//
// A worker whose module crashed (a WebAssembly trap leaves the module
// unusable) or that stopped breaks the pool: every worker ends at once, with
// its memory, and what was asked of them fails with that error, as does every
// patch and draw asked before the next load. That load starts a fresh pool.
export class MesherClient {
  // How many workers to run on a machine with `cores` logical cores: one
  // fewer than the cores, at least 2 and at most 8.
  static poolSize(cores) {
    return Math.min(8, Math.max(2, cores - 1));
  }

  // `spawn()` makes a worker; a pool is the world's and `size` - 1 drawing ones.
  constructor(spawn, size) {
    this.spawn = spawn;
    this.size = size;
    this.pool = null;
    // Why there is no pool: what patches and draws fail with until a load.
    this.failure = new Error('No world is loaded');
    this.requests = new Map();
    this.next = 0;
    // Operations not sent yet, in the order they were asked.
    this.queue = [];
    // The module and pack of the last load, which draws are made with.
    this.assets = null;
  }

  start() {
    const pool = { world: null, drawers: [] };
    const make = () => {
      const worker = this.spawn();
      worker.onmessage = ({ data }) => {
        if (this.pool !== pool) return;
        if (data.broken) return this.fail(new Error(data.error));
        const { id, ...answer } = data;
        const request = this.requests.get(id);
        this.requests.delete(id);
        if (answer.error) request.reject(new Error(answer.error));
        else request.resolve(answer);
      };
      worker.onerror = (event) => {
        if (this.pool === pool) this.fail(new Error(`The mesher stopped: ${event.message}`));
      };
      return worker;
    };
    pool.world = make();
    pool.drawers = Array.from({ length: this.size - 1 }, () => ({ worker: make(), busy: false }));
    this.pool = pool;
  }

  // The pool is broken: its workers end and what was asked of them fails.
  fail(error) {
    const { world, drawers } = this.pool;
    for (const worker of [world, ...drawers.map((entry) => entry.worker)]) worker.terminate();
    this.pool = null;
    this.failure = error;
    for (const request of this.requests.values()) request.reject(error);
    this.requests.clear();
    this.pump();
  }

  send(worker, message, transfer = []) {
    return new Promise((resolve, reject) => {
      const id = ++this.next;
      this.requests.set(id, { resolve, reject });
      worker.postMessage({ ...message, id }, transfer);
    });
  }

  // Sends the queue's operations in order: a load or patch to the world's
  // worker, a draw once a drawing worker is free (its snapshot from the
  // world's worker, then the tile from the drawing one). Without a pool, a
  // load starts one and a patch or draw fails.
  pump() {
    while (this.queue.length) {
      const operation = this.queue[0];
      if (!this.pool) {
        if (!operation.load) {
          this.queue.shift();
          operation.reject(this.failure);
          continue;
        }
        this.start();
      }
      const pool = this.pool;
      if (operation.draw) {
        const drawer = pool.drawers.find((entry) => !entry.busy);
        if (!drawer) return;
        this.queue.shift();
        drawer.busy = true;
        const { wasm, pack } = this.assets;
        this.send(pool.world, { type: 'snapshot', x: operation.draw[0], z: operation.draw[1] })
          .then(({ bytes }) => this.send(drawer.worker, { type: 'draw', wasm, pack, snapshot: bytes }, [bytes]))
          .then(({ bytes }) => operation.resolve(bytes), operation.reject)
          .finally(() => {
            drawer.busy = false;
            this.pump();
          });
        continue;
      }
      this.queue.shift();
      if (operation.load) {
        const { wasm, pack } = operation.message;
        this.assets = { wasm, pack };
        // The drawing workers make their module ready while the world loads.
        for (const { worker } of pool.drawers) this.send(worker, { type: 'prepare', wasm, pack }).catch(() => {});
      }
      this.send(pool.world, operation.message, operation.transfer).then(operation.resolve, operation.reject);
    }
  }

  // Queues an operation: { message, transfer } for the world's worker
  // (`load` for a load), or { draw: [x, z] }.
  enqueue(operation) {
    return new Promise((resolve, reject) => {
      this.queue.push({ ...operation, resolve, reject });
      this.pump();
    });
  }

  // Makes `view`'s blueprint the world to draw (see Mesher.finish).
  load(view, compare) {
    return this.enqueue({ load: true, message: { type: 'load', wasm: view.wasm, pack: view.pack, blueprint: view.blueprint, mask: view.mask, compare } });
  }

  // Applies a patch (a Uint8Array, copied and transferred) to the world
  // (see Mesher.patch): { tiles, changed }.
  patch(bytes) {
    const patch = new Uint8Array(bytes).buffer;
    return this.enqueue({ message: { type: 'patch', patch }, transfer: [patch] });
  }

  draw(x, z) {
    return this.enqueue({ draw: [x, z] });
  }
}

// Geometry tiles arrive indexed (the worker shares each quad's repeated
// corners). BlueMap's PRBM loader, which never meets an indexed tile from
// BlueMap itself, reads the material groups from where the indices begin;
// they follow the indices, so they are read from there. The indices are a
// view into the tile's bytes, which places them exactly.
function readGroupsAfterIndices(geometry, data) {
  const index = geometry.index.array;
  if (index.buffer !== data) throw new Error('Tile indices are not a view of the tile');
  const view = new DataView(data);
  const groups = [];
  for (let at = Math.ceil((index.byteOffset + index.byteLength) / 4) * 4; at + 4 <= data.byteLength; at += 12) {
    const materialIndex = view.getInt32(at, true);
    if (materialIndex === -1) break;
    groups.push({ materialIndex, start: view.getInt32(at + 4, true), count: view.getInt32(at + 8, true) });
  }
  geometry.groups = groups;
  return geometry;
}

// Keep BlueMap's meshes, materials, lighting, and camera. Only tile discovery
// and delivery change: the mesher draws each hires tile when BlueMap loads
// it, from the world of the view on show. BlueMap's lowres layer is never
// shown (bluemap-view.mjs keeps every build in hires range), so none is made.
export class BlueMapTileSync {
  constructor(app, mesher) {
    this.viewer = app.mapViewer;
    this.mesher = mesher;
    // Tile hash -> { x, z, version }: version is the refresh that last
    // changed the tile's bytes.
    this.assets = new Map();
    this.refreshes = 0;
    // A refresh or patch in flight: each reads and replaces `assets` and the
    // loaded tiles across its awaits, so they run one at a time (the bridge
    // queues them) and one asked during another rejects.
    this.busy = false;
    for (const lowres of this.viewer.map.lowresTileManager) {
      lowres.removeAllTiles();
      lowres.loadNextTile = () => false;
    }
    const manager = this.manager = this.viewer.map.hiresTileManager;
    manager.removeAllTiles();
    const loader = manager.tileLoader;
    const parse = loader.bufferGeometryLoader.parse.bind(loader.bufferGeometryLoader);
    loader.bufferGeometryLoader.parse = (data) => readGroupsAfterIndices(parse(data), data);
    loader.fileLoader.load = (url, onLoad, _progress, onError) => {
      const [, x, z] = url.match(TILE).map((part) => Number(part.replaceAll('/', '')));
      this.mesher.draw(x, z).then(onLoad, onError);
    };
    // Nearest first, among the tiles the view has.
    manager.loadNextTile = () => {
      if (manager.unloaded) return false;
      let closest = null;
      let distance = Infinity;
      for (const asset of this.assets.values()) {
        if (!this.visible(asset) || manager.tiles.has(hashTile(asset.x, asset.z))) continue;
        const d = (asset.x - manager.centerTile.x) ** 2 + (asset.z - manager.centerTile.y) ** 2;
        if (d < distance) { closest = asset; distance = d; }
      }
      if (!closest) return false;
      const loaded = manager.tryLoadTile(closest.x, closest.z);
      manager.tiles.get(hashTile(closest.x, closest.z)).mcbenchVersion = closest.version;
      return loaded;
    };
  }

  visible(asset) {
    return Math.abs(asset.x - this.manager.centerTile.x) <= this.manager.viewDistanceX
      && Math.abs(asset.z - this.manager.centerTile.y) <= this.manager.viewDistanceZ;
  }

  // Shows `view` ({ blueprint, mask, wasm, pack }: its blueprint's address,
  // render mask and the mesher's assets). `fresh`: another chat's world
  // replaces this one. Its tiles share nothing with the old ones, so the old
  // ones go at once and BlueMap streams the new build nearest-first, as on a
  // first load. Otherwise only tiles the mesher reports changed are drawn
  // again, and loaded ones are swapped in together once all are drawn.
  refresh(view, options) {
    return this.alone(() => this.load(view, options));
  }

  async alone(work) {
    if (this.busy) throw new Error('A refresh or patch of the tiles is already running');
    this.busy = true;
    try {
      return await work();
    } finally {
      this.busy = false;
    }
  }

  async load(view, { fresh = false } = {}) {
    if (fresh) {
      this.assets = new Map();
      this.manager.removeAllTiles();
    }
    const { tiles, changed } = await this.mesher.load(view, !fresh);
    this.refreshes += 1;
    const redrawn = new Set(changed.map(([x, z]) => hashTile(x, z)));
    const assets = new Map();
    for (const [x, z] of tiles) {
      const key = hashTile(x, z);
      const before = this.assets.get(key);
      assets.set(key, { x, z, version: before && !redrawn.has(key) ? before.version : this.refreshes });
    }
    this.assets = assets;
    await this.swap();

    // Reveal a fresh iframe once visible hires geometry can paint. BlueMap
    // streams the remaining tiles progressively; waiting for all of them
    // leaves large builds hidden behind the loading mask.
    // Replacements above still commit atomically, and empty views clear
    // every old tile before acknowledgement.
    const deadline = performance.now() + 30_000;
    for (;;) {
      let visible = 0;
      let loaded = 0;
      for (const asset of assets.values()) {
        if (!this.visible(asset)) continue;
        const tile = this.manager.tiles.get(hashTile(asset.x, asset.z));
        if (tile && !tile.loading && !tile.loaded) throw new Error(`Map tile could not be loaded: ${asset.x},${asset.z}`);
        visible++;
        if (tile?.loaded) loaded++;
      }
      if (visible === 0 || loaded > 0) break;
      if (performance.now() > deadline) throw new Error('No visible map tile loaded within 30 seconds');
      await new Promise((resolve) => setTimeout(resolve, 16));
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  }

  // Applies live world patches (web/live-patch.mjs), in order, to the
  // world on show, which grows and shrinks with them: tiles it no longer has
  // go, and every tile they change or add is drawn, swapped in together.
  patch(patches) {
    return this.alone(() => this.apply(patches));
  }

  async apply(patches) {
    const redrawn = new Set();
    let tiles = null;
    for (const bytes of patches) {
      const result = await this.mesher.patch(bytes);
      for (const [x, z] of result.changed) redrawn.add(hashTile(x, z));
      tiles = result.tiles;
    }
    if (!tiles || (!redrawn.size && tiles.length === this.assets.size && tiles.every(([x, z]) => this.assets.has(hashTile(x, z))))) return;
    this.refreshes += 1;
    const assets = new Map();
    for (const [x, z] of tiles) {
      const key = hashTile(x, z);
      const before = this.assets.get(key);
      assets.set(key, { x, z, version: before && !redrawn.has(key) ? before.version : this.refreshes });
    }
    this.assets = assets;
    await this.swap();
  }

  // Draws every loaded tile whose version is not its asset's, and swaps the
  // drawn ones in together; drops tiles the view no longer has.
  async swap() {
    const assets = this.assets;
    const manager = this.manager;
    const replacements = [];
    const removals = [];
    for (const [key, previous] of manager.tiles) {
      const asset = assets.get(key);
      if (!asset) removals.push({ key, previous });
      else if (previous.mcbenchVersion !== asset.version || (!previous.loading && !previous.loaded)) {
        const next = new previous.constructor(asset.x, asset.z, () => {}, () => {});
        next.mcbenchVersion = asset.version;
        replacements.push({ key, previous, next });
      }
    }
    // Prepare replacements before removing their old geometry. Publishing the
    // completed set together avoids blanking the scene on a small update.
    let cursor = 0;
    const prepared = await Promise.allSettled(Array.from({ length: Math.min(8, replacements.length) }, async () => {
      while (cursor < replacements.length) {
        const { next } = replacements[cursor++];
        await next.load(manager.tileLoader);
        if (!next.loaded) throw new Error(`Map tile could not be decoded: ${next.x},${next.z}`);
      }
    }));
    const failure = prepared.find((result) => result.status === 'rejected');
    if (failure) {
      for (const { next } of replacements) next.unload();
      throw failure.reason;
    }
    for (const { key, previous } of removals) {
      if (manager.tiles.get(key) !== previous) continue;
      previous.unload();
      manager.tiles.delete(key);
    }
    for (const { key, previous, next } of replacements) {
      // Camera movement can unload or reload this position while drawing.
      if (manager.tiles.get(key) !== previous) { next.unload(); continue; }
      previous.unload();
      next.onLoad = manager.handleLoadedTile;
      next.onUnload = manager.handleUnloadedTile;
      manager.tiles.set(key, next);
      manager.handleLoadedTile(next);
    }
    this.viewer.updateLoadedMapArea();
  }
}
