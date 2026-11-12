'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const { Worker } = require('node:worker_threads');

const { MODULE, SOURCES_HASH, sourcesHash } = require('../scripts/build-renderer');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');
const { mesherPack } = require('./helpers/mesher-pack');
const { communityBuilds, communityBlocks, bakedPack } = require('./helpers/community');
const { expand } = require('./helpers/prbm');

const WASM = fs.readFileSync(MODULE);
const PACK = mesherPack();
const dataUrl = (bytes) => `data:application/octet-stream;base64,${Buffer.from(bytes).toString('base64')}`;

// A blueprint of blocks given as "x,y,z" -> state name, at Minecraft heights.
function blueprint(blocks) {
  return blocksBlueprint([...blocks].map(([key, name]) => [...key.split(',').map(Number), name]));
}

// A deterministic pseudo-random sequence.
function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

const NAMES = ['minecraft:stone', 'minecraft:glass', 'minecraft:glowstone', 'minecraft:lamp[lit=true]', 'minecraft:lamp'];

// Rooms, roofs and lights over a few chunks either side of the origin.
function sampleBlocks() {
  const blocks = new Map();
  const next = random(11);
  for (let x = -20; x < 40; x += 1) for (let z = -12; z < 30; z += 1) {
    blocks.set(`${x},64,${z}`, 'minecraft:stone');
    if (next() < 0.3) blocks.set(`${x},${65 + Math.floor(next() * 6)},${z}`, NAMES[Math.floor(next() * NAMES.length)]);
  }
  for (let x = 0; x < 12; x += 1) for (let z = 0; z < 12; z += 1) blocks.set(`${x},72,${z}`, 'minecraft:glass');
  return blocks;
}

const bounds = (blocks) => {
  const points = [...blocks.keys()].map((key) => key.split(',').map(Number));
  return { min: [0, 1, 2].map((i) => Math.min(...points.map((p) => p[i]))), max: [0, 1, 2].map((i) => Math.max(...points.map((p) => p[i]))) };
};

// Every tile of the loaded world, drawn directly by the module.
function drawAll(mesher, tiles) {
  return new Map(tiles.map(([x, z]) => [`${x},${z}`, Buffer.from(mesher.draw(x, z))]));
}

test('the committed mesher module is built from the current renderer sources', () => {
  assert.equal(fs.readFileSync(SOURCES_HASH, 'utf8').trim(), sourcesHash(),
    'renderer/ changed since the module was built: run node scripts/build-renderer.js');
});

// The real worker file (web/mesher-worker.mjs) in pools of `count` Node (the world's and `count` - 1 drawing)
// worker threads, each one's `self` the thread's port, driven through the
// bridge's MesherClient. `spawned` collects every worker made.
async function nodeWorker(t, count = 2, spawned = []) {
  const { MesherClient } = await import('../web/bluemap-tiles.mjs');
  return new MesherClient(() => {
    const worker = new Worker(`
      const { parentPort } = require('node:worker_threads');
      globalThis.self = globalThis;
      self.postMessage = (message, transfer) => parentPort.postMessage(message, transfer);
      parentPort.on('message', (data) => self.onmessage({ data }));
      import(${JSON.stringify(path.join(__dirname, '..', 'web', 'mesher-worker.mjs'))});
    `, { eval: true });
    const entry = { ended: false };
    worker.on('exit', () => { entry.ended = true; });
    spawned.push(entry);
    t.after(() => worker.terminate());
    return {
      postMessage: (message, transfer) => worker.postMessage(message, transfer),
      terminate: () => worker.terminate(),
      set onmessage(listener) { worker.on('message', (data) => listener({ data })); },
      set onerror(listener) { worker.on('error', (error) => listener({ message: error.message })); },
    };
  }, count);
}

test('the viewer draws exactly the module\'s tiles: the worker\'s indexed tiles expand to the module\'s bytes', async (t) => {
  const { Mesher } = await import('../web/mesher.mjs');
  const blocks = sampleBlocks();
  const bytes = blueprint(blocks);
  const mask = renderMask(bounds(blocks));
  const mesher = await Mesher.create(WASM, PACK);
  const { tiles } = mesher.load(bytes, mask, false);
  assert.ok(tiles.length >= 4, `the sample spans several tiles (${tiles.length})`);
  const expected = drawAll(mesher, tiles);
  // Tiles over a column's edge hold only what crosses it, often nothing.
  assert.ok([...expected.values()].filter((tile) => tile.length > 100).length >= 4, 'tiles have faces');

  const client = await nodeWorker(t);
  const view = { wasm: dataUrl(WASM), pack: dataUrl(PACK), blueprint: dataUrl(bytes), mask };
  const loaded = await client.load(view, true);
  assert.deepEqual(loaded.tiles, tiles);
  assert.deepEqual(loaded.changed, tiles, 'a first world draws every tile');
  for (const [x, z] of tiles) {
    const drawn = Buffer.from(await client.draw(x, z));
    const raw = expected.get(`${x},${z}`);
    assert.equal(drawn[1] >> 7 & 1, 1, 'tiles arrive indexed');
    const viewer = expand(drawn);
    const module = expand(raw);
    assert.deepEqual(viewer.corners, module.corners, `tile ${x},${z}: every corner the GPU draws has the module's bytes`);
    assert.equal(viewer.groups, module.groups, `tile ${x},${z}: the same material groups`);
  }
  await assert.rejects(client.load({ ...view, blueprint: dataUrl(Buffer.from([0, 0, 0, 9])) }, true), /The mesher failed: The blueprint ends early/);
  const again = await client.load(view, true);
  assert.deepEqual(again.changed, [], 'a failed load keeps the world it would have replaced');
});

test('an edit redraws only tiles whose bytes can change; every other tile is byte-identical', async () => {
  const { Mesher } = await import('../web/mesher.mjs');
  const mesher = await Mesher.create(WASM, PACK);
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  let { tiles } = mesher.load(blueprint(blocks), mask, true);
  let drawn = drawAll(mesher, tiles);
  const next = random(5);
  let skipped = 0;
  for (let edit = 0; edit < 24; edit += 1) {
    // Single blocks and roofs, lights and holes, anywhere in the build.
    const x = -24 + Math.floor(next() * 68);
    const z = -16 + Math.floor(next() * 50);
    const y = 64 + Math.floor(next() * 10);
    const roll = next();
    if (roll < 0.2) for (let dx = 0; dx < 6; dx += 1) for (let dz = 0; dz < 6; dz += 1) blocks.set(`${x + dx},${y + 3},${z + dz}`, 'minecraft:stone');
    else if (roll < 0.5) blocks.delete([...blocks.keys()][Math.floor(next() * blocks.size)]);
    else blocks.set(`${x},${y},${z}`, NAMES[Math.floor(next() * NAMES.length)]);
    const loaded = mesher.load(blueprint(blocks), mask, true);
    const after = drawAll(mesher, loaded.tiles);
    const changed = new Set(loaded.changed.map(([cx, cz]) => `${cx},${cz}`));
    for (const [key, bytes] of after) {
      if (changed.has(key)) continue;
      skipped += 1;
      assert.ok(drawn.get(key)?.equals(bytes), `edit ${edit}: tile ${key} was not reported but its bytes changed`);
    }
    ({ tiles } = loaded);
    drawn = after;
  }
  assert.ok(skipped > 24 * tiles.length / 3, `most tiles are not redrawn (${skipped} kept)`);
  const moved = mesher.load(blueprint(blocks), { ...mask, minX: mask.minX - 16 }, true);
  assert.deepEqual(moved.changed, moved.tiles, 'a new render mask redraws every tile');
});

// A blueprint's palette as state names, entry 0 first.
function paletteOf(bytes) {
  let at = 4;
  const palette = [];
  const text = () => { const n = bytes.readUInt16BE(at); at += 2; const v = bytes.toString('utf8', at, at + n); at += n; return v; };
  for (let entries = bytes.readInt32BE(0); entries > 0; entries -= 1) {
    const id = text();
    const pairs = [];
    for (let n = bytes.readInt32BE(at), i = (at += 4, 0); i < n; i += 1) pairs.push(`${text()}=${text()}`);
    palette.push(pairs.length ? `${id}[${pairs.join(',')}]` : id);
  }
  return palette;
}

// A patch (the live contract's format) to the stream whose palette is
// `palette` (appended to by the names it adds): the blocks `edits` ([x, y, z,
// state name or null for air]) over the world `blocks`, which has them
// already.
function patchBytes(palette, blocks, edits) {
  const parts = [];
  const int = (value) => { const b = Buffer.alloc(4); b.writeInt32BE(value); parts.push(b); };
  const text = (value) => { const b = Buffer.from(value); const n = Buffer.alloc(2); n.writeUInt16BE(b.length); parts.push(n, b); };
  const added = [];
  const indices = edits.map(([, , , name]) => {
    if (name === null) return 0;
    let at = [...palette, ...added].indexOf(name);
    if (at < 0) { added.push(name); at = palette.length + added.length - 1; }
    return at;
  });
  int(added.length);
  for (const name of added) {
    const open = name.indexOf('[');
    text(open < 0 ? name : name.slice(0, open));
    const pairs = open < 0 ? [] : name.slice(open + 1, -1).split(',').map((pair) => pair.split('='));
    int(pairs.length);
    for (const [key, value] of pairs) { text(key); text(value); }
  }
  const { min, max } = bounds(blocks);
  for (const value of [...min, ...max]) int(value);
  int(new Set([...blocks.keys()].map((key) => key.split(',').filter((_, i) => i !== 1).map((v) => Number(v) >> 4).join())).size);
  int(edits.length);
  edits.forEach(([x, y, z], i) => {
    for (const value of [x, y, z]) int(value);
    const b = Buffer.alloc(2); b.writeUInt16BE(indices[i]); parts.push(b);
  });
  palette.push(...added);
  return Buffer.concat(parts);
}

const columnsOf = (blocks) => new Set([...blocks.keys()].map((key) => { const [x, , z] = key.split(',').map(Number); return `${x >> 4},${z >> 4}`; }));
const sameSet = (a, b) => a.size === b.size && [...a].every((value) => b.has(value));

// Patches `world` through `steps` random batches of edits; after each, a
// fresh load of the same blocks must have the patched world's tiles and draw
// every one with its bytes, and every tile whose bytes changed must be among
// those the patch reports (the very tiles a fresh load reports while the
// bounds and built columns stay). Now and then a block lands past the bounds
// (above, below as falling water does, beside, in an unbuilt chunk column)
// and a later batch takes it away again: the world grows and shrinks.
// `world`: { blocks, names, mask }; `make`: the world's mesher factory.
// `pick(next)`, when given, picks each edit's position ([x, y, z]) and
// `choose(key, roll, blocks)` its state name (null for air; undefined to
// leave the position alone).
async function patchSequence(t, { blocks, patchNames, mask, steps, seed, pack = PACK, wasm = WASM, area, heights, pick, choose }) {
  const { Mesher } = await import('../web/mesher.mjs');
  const patched = await Mesher.create(wasm, pack);
  const fresh = await Mesher.create(wasm, pack);
  const next = random(seed);
  const bytes = blueprint(blocks);
  let palette = paletteOf(bytes);
  let { tiles } = patched.load(bytes, mask, false);
  fresh.load(bytes, mask, false);
  let drawn = drawAll(patched, tiles);
  let applied = 0;
  let moved = 0;
  let nothing = 0;
  let grown = [];
  for (let step = 0; step < steps; step += 1) {
    const before = new Map(blocks);
    const edits = [];
    const touched = new Set();
    const count = step % 5 === 0 ? 40 + Math.floor(next() * 80) : 1 + Math.floor(next() * 5);
    const [cx, cz] = [area.x + next() * area.w, area.z + next() * area.d];
    for (let i = 0; i < count; i += 1) {
      const wide = next() < 0.3;
      let x = Math.floor(wide ? area.x + next() * area.w : cx + (next() - 0.5) * 10);
      let z = Math.floor(wide ? area.z + next() * area.d : cz + (next() - 0.5) * 10);
      let y = heights.min + Math.floor(next() * (heights.max - heights.min + 1));
      if (pick) [x, y, z] = pick(next);
      const key = `${x},${y},${z}`;
      if (touched.has(key)) continue;
      touched.add(key);
      const roll = next();
      let name;
      if (choose) name = choose(key, roll, blocks);
      else if (roll < 0.25) name = null;
      else if (roll < 0.4) name = patchNames.find((n) => n.includes('lit=true') || n.includes('glowstone')) || patchNames[0];
      else name = patchNames[Math.floor(next() * patchNames.length)];
      if (name === undefined) continue;
      if (name === null) blocks.delete(key); else blocks.set(key, name);
      edits.push([x, y, z, name]);
    }
    // Opaque roofs over a lit area now and then.
    if (step % 6 === 3) {
      const x = Math.floor(cx);
      const z = Math.floor(cz);
      for (let dx = 0; dx < 7; dx += 1) for (let dz = 0; dz < 7; dz += 1) {
        const key = `${x + dx},${heights.max},${z + dz}`;
        if (touched.has(key)) continue;
        touched.add(key);
        blocks.set(key, 'minecraft:stone');
        edits.push([x + dx, heights.max, z + dz, 'minecraft:stone']);
      }
    }
    // Past the bounds, and later away again.
    if (step % 4 === 1) {
      const x = Math.floor(cx);
      const z = Math.floor(cz);
      const out = [[x, heights.max + 2 + Math.floor(next() * 20), z], [x, Math.max(-64, heights.min - 2 - Math.floor(next() * 40)), z], [area.x + area.w + 3 + Math.floor(next() * 30), heights.min, z], [x, heights.min, area.z - 3 - Math.floor(next() * 30)]][Math.floor(next() * 4)];
      const key = out.join(',');
      if (!touched.has(key)) {
        touched.add(key);
        blocks.set(key, 'minecraft:glowstone');
        edits.push([...out, 'minecraft:glowstone']);
        grown.push(key);
      }
    } else if (step % 4 === 3) {
      for (const key of grown) {
        if (touched.has(key) || !blocks.has(key)) continue;
        touched.add(key);
        blocks.delete(key);
        edits.push([...key.split(',').map(Number), null]);
      }
      grown = [];
    }
    // Entries that change nothing: the block (or air) already there.
    for (let i = 0; i < 3; i += 1) {
      const key = [...blocks.keys()][Math.floor(next() * blocks.size)];
      if (touched.has(key)) continue;
      touched.add(key);
      edits.push([...key.split(',').map(Number), blocks.get(key)]);
    }
    const air = `${Math.floor(area.x + next() * area.w)},${heights.max + 5},${Math.floor(area.z + next() * area.d)}`;
    if (!touched.has(air) && !blocks.has(air)) edits.push([...air.split(',').map(Number), null]);

    const still = sameSet(columnsOf(blocks), columnsOf(before)) && JSON.stringify(bounds(blocks)) === JSON.stringify(bounds(before));
    const result = patched.patch(patchBytes(palette, blocks, edits));
    const reference = fresh.load(blueprint(blocks), mask, true);
    const label = `step ${step}`;
    applied += 1;
    moved += still ? 0 : 1;
    if (!result.changed.length) nothing += 1;
    assert.deepEqual(result.tiles, reference.tiles, `${label}: the tiles are a fresh load's`);
    if (still) assert.deepEqual(result.changed, reference.changed, `${label}: the changed tiles are a fresh load's`);
    tiles = result.tiles;
    const after = drawAll(patched, tiles);
    const expected = drawAll(fresh, tiles);
    const changed = new Set(result.changed.map(([x, z]) => `${x},${z}`));
    for (const [key, tile] of expected) {
      assert.ok(after.get(key).equals(tile), `${label}: tile ${key} differs from a fresh load's`);
      if (!drawn.get(key)?.equals(tile)) assert.ok(changed.has(key), `${label}: tile ${key} changed but was not reported`);
    }
    drawn = after;
  }
  return { applied, moved, nothing };
}

const WORLD_NAMES = ['minecraft:stone', 'minecraft:glass', 'minecraft:glowstone', 'minecraft:lamp[lit=true]', 'minecraft:lamp'];

// Towers with glass roofs and lamps inside, over a few chunks.
function towerBlocks() {
  const blocks = new Map();
  const next = random(23);
  for (let tower = 0; tower < 14; tower += 1) {
    const [x0, z0] = [Math.floor(next() * 50) - 10, Math.floor(next() * 40) - 8];
    const height = 4 + Math.floor(next() * 8);
    for (let x = x0; x < x0 + 4; x += 1) for (let z = z0; z < z0 + 4; z += 1) {
      for (let y = 64; y < 64 + height; y += 1) {
        const wall = x === x0 || x === x0 + 3 || z === z0 || z === z0 + 3;
        if (wall) blocks.set(`${x},${y},${z}`, 'minecraft:stone');
      }
      blocks.set(`${x},${64 + height},${z}`, next() < 0.5 ? 'minecraft:glass' : 'minecraft:stone');
    }
    blocks.set(`${x0 + 1},65,${z0 + 1}`, 'minecraft:glowstone');
  }
  blocks.set('0,64,0', 'minecraft:stone');
  return blocks;
}

// Two islands with a gap of unbuilt chunk columns between.
function islandBlocks() {
  const blocks = new Map();
  const next = random(31);
  for (const [x0, z0] of [[-20, -10], [40, 20]]) {
    for (let x = x0; x < x0 + 24; x += 1) for (let z = z0; z < z0 + 24; z += 1) {
      blocks.set(`${x},64,${z}`, 'minecraft:stone');
      if (next() < 0.25) blocks.set(`${x},${65 + Math.floor(next() * 5)},${z}`, WORLD_NAMES[Math.floor(next() * WORLD_NAMES.length)]);
    }
  }
  return blocks;
}

const BUILDS = {
  rooms: { blocks: sampleBlocks, area: { x: -22, z: -14, w: 66, d: 46 }, heights: { min: 64, max: 74 }, names: WORLD_NAMES },
  towers: { blocks: towerBlocks, area: { x: -12, z: -10, w: 56, d: 46 }, heights: { min: 64, max: 78 }, names: WORLD_NAMES },
  islands: { blocks: islandBlocks, area: { x: -22, z: -12, w: 88, d: 60 }, heights: { min: 64, max: 74 }, names: WORLD_NAMES },
};

for (const [name, build] of Object.entries(BUILDS)) {
  test(`patching the ${name} build stays identical to loading it: tiles, bytes and changed tiles, as it grows and shrinks`, async (t) => {
    const blocks = build.blocks();
    // The base lacks the lamps; patches bring them (palette growth).
    for (const [key, state] of blocks) if (state.startsWith('minecraft:lamp')) blocks.set(key, 'minecraft:stone');
    const mask = renderMask(bounds(build.blocks()));
    const { applied, moved, nothing } = await patchSequence(t, { blocks, patchNames: build.names, mask, steps: 36, seed: name.length * 977, area: build.area, heights: build.heights });
    assert.equal(applied, 36);
    assert.ok(moved >= 10, `the bounds or the built columns move (${moved} times)`);
    assert.ok(nothing < applied, 'most patches change tiles');
  });
}

// Edits that change no light (glass for air, wire power, repeater and
// comparator state) with a lamp or a light among them now and then: each
// patched world must still draw a fresh load's bytes, and the module reports
// the tiles that changed.
const SWAPS = [
  [/^minecraft:redstone_wire\[/, (name, roll) => name.replace(/power=\d+/, `power=${Math.floor(roll * 16)}`)],
  [/^minecraft:(repeater|comparator)\[/, (name) => name.replace(/powered=(true|false)/, (_, v) => `powered=${v === 'true' ? 'false' : 'true'}`)],
];

test('patches that mostly change no light stay identical to loading, with lamps among them', async (t) => {
  const base = sampleBlocks();
  for (const [key, state] of base) if (state.startsWith('minecraft:lamp')) base.set(key, 'minecraft:stone');
  const mask = renderMask(bounds(base));
  const lamp = (current) => (current === 'minecraft:lamp[lit=true]' ? 'minecraft:lamp' : 'minecraft:lamp[lit=true]');
  const choose = (key, roll, blocks) => {
    const current = blocks.get(key);
    if (roll < 0.08) return lamp(current);
    if (current === undefined) return 'minecraft:glass';
    return current === 'minecraft:glass' ? null : undefined;
  };
  const { applied } = await patchSequence(t, { blocks: base, patchNames: [], mask, steps: 30, seed: 41, area: BUILDS.rooms.area, heights: BUILDS.rooms.heights, choose });
  assert.equal(applied, 30);
});

test('patches of wire power, repeaters and lamps through the baked pack stay identical to loading community builds', async (t) => {
  const pack = bakedPack();
  const builds = communityBuilds();
  if (!pack || builds.length < 4) return t.skip('no baked pack in the data directory and community builds in MCBENCH_CORPUS');
  const chosen = [0, 1, 2, 3].map((i) => builds[Math.floor(i * (builds.length - 1) / 3)]);
  let swapped = 0;
  for (const [i, { file }] of chosen.entries()) {
    const blocks = communityBlocks(file);
    const { min, max } = bounds(blocks);
    const wired = [...blocks].filter(([, state]) => SWAPS.some(([pattern]) => pattern.test(state))).map(([key]) => key.split(',').map(Number));
    if (!wired.length) continue;
    const pick = (next) => wired[Math.floor(next() * wired.length)];
    const choose = (key, roll, current) => {
      const state = current.get(key);
      if (roll < 0.05) return 'minecraft:redstone_lamp[lit=true]';
      const swap = SWAPS.find(([pattern]) => pattern.test(state));
      if (!swap) return undefined;
      swapped += 1;
      return swap[1](state, roll);
    };
    const { applied } = await patchSequence(t, {
      blocks, patchNames: [], pack, mask: renderMask({ min, max }), steps: 8, seed: 301 + i, pick, choose,
      area: { x: min[0], z: min[2], w: max[0] - min[0] + 1, d: max[2] - min[2] + 1 }, heights: { min: min[1], max: max[1] },
    });
    assert.equal(applied, 8, path.basename(path.dirname(file)));
  }
  assert.ok(swapped > 20, `the builds hold redstone to swap (${swapped})`);
});

test('entries that change nothing change no tile; a patch past the bounds grows the world and one that takes it back shrinks it; a failed patch changes nothing', async () => {
  const { Mesher } = await import('../web/mesher.mjs');
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  const mesher = await Mesher.create(WASM, PACK);
  const bytes = blueprint(blocks);
  const palette = paletteOf(bytes);
  const { tiles } = mesher.load(bytes, mask, false);
  const drawn = drawAll(mesher, tiles);
  const same = () => { for (const [key, tile] of drawn) assert.ok(Buffer.from(mesher.draw(...key.split(',').map(Number))).equals(tile), `tile ${key} changed`); };
  const reference = await Mesher.create(WASM, PACK);
  const fresh = (world) => {
    const loaded = reference.load(blueprint(world), mask, false);
    for (const [x, z] of loaded.tiles) assert.ok(Buffer.from(mesher.draw(x, z)).equals(Buffer.from(reference.draw(x, z))), `tile ${x},${z}`);
    return loaded;
  };

  const kept = [...blocks].filter((_, i) => i % 37 === 0).map(([key, state]) => [...key.split(',').map(Number), state]);
  kept.push([3, 70, 3, null]);
  assert.deepEqual(mesher.patch(patchBytes(palette, blocks, kept)), { tiles, changed: [] });
  same();

  // Up past the top, then into a chunk column past the side, then back.
  const grown = new Map(blocks);
  grown.set('3,200,3', 'minecraft:lamp[lit=true]');
  let result = mesher.patch(patchBytes(palette, grown, [[3, 200, 3, 'minecraft:lamp[lit=true]']]));
  assert.deepEqual(result.tiles, fresh(grown).tiles);
  grown.set('90,64,3', 'minecraft:stone');
  result = mesher.patch(patchBytes(palette, grown, [[90, 64, 3, 'minecraft:stone']]));
  assert.deepEqual(result.tiles, fresh(grown).tiles);
  assert.ok(result.tiles.length > tiles.length && result.tiles.filter(([x]) => x >= 2).every((tile) => result.changed.some((t) => t.join() === tile.join())), 'the new column\'s tiles are drawn');
  const lit = new Map(blocks);
  lit.set('3,66,3', 'minecraft:lamp[lit=true]');
  result = mesher.patch(patchBytes(palette, lit, [[3, 200, 3, null], [90, 64, 3, null], [3, 66, 3, 'minecraft:lamp[lit=true]']]));
  assert.deepEqual(result.tiles, tiles, 'the world shrinks back');
  assert.ok(result.changed.length > 0);
  fresh(lit);

  // Malformed, unknown, out-of-palette and out-of-bounds patches fail and change nothing.
  const good = patchBytes([...palette], lit, [[4, 66, 3, 'minecraft:stone']]);
  const outside = Buffer.from(good);
  outside.writeInt32BE(900, outside.length - 14);
  const unknown = patchBytes([...palette], lit, [[4, 66, 3, 'minecraft:bedrock_xyz']]);
  const index = Buffer.from(good);
  index.writeUInt16BE(77, index.length - 2);
  const columns = Buffer.from(good);
  columns.writeInt32BE(99, columns.length - 22);
  for (const [bad, message] of [[good.subarray(0, good.length - 1), /The patch ends early/], [Buffer.concat([good, Buffer.from([0])]), /The patch has trailing bytes/], [outside, /outside its palette or bounds/], [unknown, /Unknown block/], [index, /outside its palette or bounds/], [columns, /builds 99 chunk columns/]]) {
    assert.throws(() => mesher.patch(bad), message);
    fresh(lit);
  }
  const empty = await Mesher.create(WASM, PACK);
  assert.throws(() => empty.patch(good), /No world is loaded/);
});

test('the worker applies patches as the module does, growing the world with them', async (t) => {
  const { Mesher } = await import('../web/mesher.mjs');
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  const bytes = blueprint(blocks);
  const palette = paletteOf(bytes);
  const client = await nodeWorker(t);
  const view = { wasm: dataUrl(WASM), pack: dataUrl(PACK), blueprint: dataUrl(bytes), mask };
  await client.load(view, false);
  const direct = await Mesher.create(WASM, PACK);
  direct.load(bytes, mask, false);
  const edited = new Map(blocks);
  edited.set('5,66,5', 'minecraft:lamp[lit=true]');
  edited.set('6,72,6', 'minecraft:stone');
  const tall = new Map(edited);
  tall.set('5,300,5', 'minecraft:stone');
  for (const patch of [patchBytes(palette, edited, [[5, 66, 5, 'minecraft:lamp[lit=true]'], [6, 72, 6, 'minecraft:stone']]), patchBytes(palette, tall, [[5, 300, 5, 'minecraft:stone']])]) {
    const answer = await client.patch(patch);
    assert.deepEqual(answer, direct.patch(patch));
    assert.ok(answer.changed.length > 0);
    assert.ok(patch.byteLength > 0, 'the caller keeps its bytes');
    for (const [x, z] of answer.tiles) assert.deepEqual(expand(Buffer.from(await client.draw(x, z))).corners, expand(Buffer.from(direct.draw(x, z))).corners, `tile ${x},${z}`);
  }
});

test('a load whose module or pack cannot be fetched keeps the world the worker holds', async (t) => {
  const { Mesher } = await import('../web/mesher.mjs');
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  const bytes = blueprint(blocks);
  const client = await nodeWorker(t);
  const view = { wasm: dataUrl(WASM), pack: dataUrl(PACK), blueprint: dataUrl(bytes), mask };
  await client.load(view, false);
  await assert.rejects(client.load({ ...view, pack: 'http://127.0.0.1:9/mcbench-pack.bin' }, true));
  const direct = await Mesher.create(WASM, PACK);
  direct.load(bytes, mask, false);
  const edited = new Map(blocks);
  edited.set('5,66,5', 'minecraft:stone');
  const patch = patchBytes(paletteOf(bytes), edited, [[5, 66, 5, 'minecraft:stone']]);
  assert.deepEqual(await client.patch(patch), direct.patch(patch));
});

test('patching community builds through the baked pack stays identical to loading them', async (t) => {
  const pack = bakedPack();
  const builds = communityBuilds();
  if (!pack || builds.length < 4) return t.skip('no baked pack in the data directory and community builds in MCBENCH_CORPUS');
  // Builds across the size range.
  const chosen = [0, 1, 2, 3].map((i) => builds[Math.floor(i * (builds.length - 1) / 3)]);
  for (const [i, { file }] of chosen.entries()) {
    const blocks = communityBlocks(file);
    const { min, max } = bounds(blocks);
    const patchNames = ['minecraft:redstone_lamp[lit=true]', 'minecraft:glowstone', 'minecraft:torch', 'minecraft:stone', 'minecraft:glass', 'minecraft:redstone_lamp[lit=false]', 'minecraft:oak_slab[type=bottom]'];
    const { applied } = await patchSequence(t, {
      blocks, patchNames, pack, mask: renderMask({ min, max }), steps: 10, seed: 101 + i,
      area: { x: min[0], z: min[2], w: max[0] - min[0] + 1, d: max[2] - min[2] + 1 }, heights: { min: min[1], max: max[1] },
    });
    assert.equal(applied, 10, path.basename(path.dirname(file)));
  }
});

test('a pool of workers draws every tile with the module\'s bytes, through a load and a patch', async (t) => {
  const { Mesher } = await import('../web/mesher.mjs');
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  const bytes = blueprint(blocks);
  const palette = paletteOf(bytes);
  const client = await nodeWorker(t, 3);
  const view = { wasm: dataUrl(WASM), pack: dataUrl(PACK), blueprint: dataUrl(bytes), mask };
  const direct = await Mesher.create(WASM, PACK);
  const loaded = await client.load(view, true);
  assert.deepEqual(loaded.tiles, direct.load(bytes, mask, false).tiles);
  // Asked together, the draws run in parallel on the workers.
  const same = async () => {
    const drawn = await Promise.all(loaded.tiles.map(([x, z]) => client.draw(x, z)));
    loaded.tiles.forEach(([x, z], i) => assert.ok(Buffer.from(direct.drawIndexed(x, z)).equals(Buffer.from(drawn[i])), `tile ${x},${z}`));
  };
  await same();
  const edited = new Map(blocks);
  edited.set('5,66,5', 'minecraft:lamp[lit=true]');
  edited.set('6,72,6', 'minecraft:stone');
  const patch = patchBytes([...palette], edited, [[5, 66, 5, 'minecraft:lamp[lit=true]'], [6, 72, 6, 'minecraft:stone']]);
  const answer = await client.patch(patch);
  assert.deepEqual(answer.changed, direct.patch(patch).changed);
  assert.ok(answer.changed.length > 0);
  assert.ok(patch.byteLength > 0, 'the caller keeps its bytes');
  await same();
  const tall = new Map(edited);
  tall.set('5,300,5', 'minecraft:stone');
  const grown = patchBytes([...palette, 'minecraft:lamp[lit=true]'], tall, [[5, 300, 5, 'minecraft:stone']]);
  assert.deepEqual(await client.patch(grown), direct.patch(grown));
  await same();
  const again = await client.load({ ...view, blueprint: dataUrl(blueprint(tall)) }, true);
  assert.deepEqual(again.changed, [], 'the world\'s worker holds the patched world');
});

// Workers that answer when the test says: `workers` holds every worker the
// pool made (the world's first, then the drawing ones), `sent` what each was
// asked and `ended` whether it was ended.
async function fakeWorkers(count) {
  const { MesherClient } = await import('../web/bluemap-tiles.mjs');
  const workers = [];
  const client = new MesherClient(() => {
    const worker = { sent: [], ended: false, postMessage(message) { this.sent.push(message); }, terminate() { this.ended = true; } };
    workers.push(worker);
    return worker;
  }, count);
  const answer = (worker, answers, message = worker.sent.at(-1)) => worker.onmessage({ data: { id: message.id, ...answers } });
  // Answers the drawing workers' 'prepare' requests.
  const prepared = (drawers) => { for (const worker of drawers) for (const message of worker.sent.filter((m) => m.type === 'prepare')) answer(worker, {}, message); };
  return { client, workers, answer, prepared };
}

const FAKE_VIEW = { wasm: 'w', pack: 'p', blueprint: 'b', mask: {} };
const tick = () => new Promise((resolve) => setImmediate(resolve));

test('the pool sends loads, patches and snapshots to the world\'s worker and each tile to a free drawing worker, in the order they were asked', async () => {
  const { MesherClient } = await import('../web/bluemap-tiles.mjs');
  assert.deepEqual([1, 2, 3, 4, 9, 10, 64].map((cores) => MesherClient.poolSize(cores)), [2, 2, 2, 3, 8, 8, 8]);

  const { client, workers, answer, prepared } = await fakeWorkers(3);
  await assert.rejects(client.draw(0, 0), /No world is loaded/);
  assert.equal(workers.length, 0, 'the first load starts the pool');
  const loaded = client.load(FAKE_VIEW, true);
  const [world, a, b] = workers;
  assert.equal(workers.length, 3, 'a pool of three: the world\'s worker and two drawing ones');
  assert.deepEqual([world.sent.map((m) => m.type), a.sent.map((m) => m.type), b.sent.map((m) => m.type)], [['load'], ['prepare'], ['prepare']]);
  assert.deepEqual([a.sent[0].wasm, a.sent[0].pack], ['w', 'p'], 'the drawing workers ready the module while the world loads');
  prepared([a, b]);
  answer(world, { tiles: [], changed: [] });
  await loaded;
  const events = [];
  const tell = (name) => (value) => events.push(name);
  const draws = [client.draw(0, 0), client.draw(1, 0), client.draw(2, 0)].map((draw, i) => draw.then(tell(`draw ${i}`)));
  assert.deepEqual(world.sent.slice(1).map((m) => [m.type, m.x]), [['snapshot', 0], ['snapshot', 1]], 'a snapshot for each free drawing worker; the third draw waits');
  const patched = client.patch(new Uint8Array([1, 2, 3])).then(tell('patch'));
  const late = client.draw(3, 0).then(tell('draw 3'));
  assert.equal(world.sent.length, 3, 'the patch waits behind the draw asked before it');
  const snapshot = new ArrayBuffer(4);
  answer(world, { bytes: snapshot }, world.sent[2]);
  await tick();
  assert.equal(b.sent.at(-1).type, 'draw');
  assert.equal(b.sent.at(-1).snapshot, snapshot, 'the snapshot goes on to the drawing worker');
  assert.deepEqual([b.sent.at(-1).wasm, b.sent.at(-1).pack], ['w', 'p']);
  answer(b, { bytes: new ArrayBuffer(0) });
  await tick();
  assert.deepEqual(world.sent.slice(3).map((m) => [m.type, m.x]), [['snapshot', 2], ['patch', undefined]], 'the freed worker takes the third draw; the patch follows its snapshot');
  answer(world, { bytes: new ArrayBuffer(0) }, world.sent[1]);
  answer(world, { bytes: new ArrayBuffer(0) }, world.sent[3]);
  await tick();
  answer(a, { bytes: new ArrayBuffer(0) });
  await tick();
  answer(b, { bytes: new ArrayBuffer(0) });
  await tick();
  answer(world, { tiles: [], changed: [[0, 0]] }, world.sent[4]);
  await Promise.all([...draws, patched]);
  assert.deepEqual(world.sent.slice(4).map((m) => m.type), ['patch', 'snapshot'], 'a draw asked after the patch is cut after it');
  answer(world, { bytes: new ArrayBuffer(0) });
  await tick();
  answer(a, { bytes: new ArrayBuffer(0) });
  await late;
  assert.deepEqual(events, ['draw 1', 'draw 0', 'draw 2', 'patch', 'draw 3']);
});

test('the pool fails a load the world\'s worker fails, and ends when one stops', async () => {
  let pool = await fakeWorkers(3);
  let load = pool.client.load(FAKE_VIEW, true);
  pool.answer(pool.workers[0], { error: 'The mesher failed: The blueprint ends early' });
  await assert.rejects(load, /The blueprint ends early/);
  assert.ok(pool.workers.every((worker) => !worker.ended), 'a failure the module reports keeps the pool');

  pool = await fakeWorkers(3);
  load = pool.client.load(FAKE_VIEW, true);
  const draw = pool.client.draw(0, 0);
  pool.workers[2].onerror({ message: 'out of memory' });
  await assert.rejects(load, /The mesher stopped: out of memory/);
  await assert.rejects(draw, /The mesher stopped: out of memory/);
  await assert.rejects(pool.client.draw(1, 1), /The mesher stopped/);
  assert.ok(pool.workers.every((worker) => worker.ended), 'a worker that stops ends the pool');
});

test('a crashed module ends its pool: what was asked of it fails, nothing more reaches it, and the next load starts a fresh pool', async () => {
  const { client, workers, answer, prepared } = await fakeWorkers(3);
  let load = client.load(FAKE_VIEW, true);
  prepared(workers.slice(1));
  answer(workers[0], { tiles: [[0, 0]], changed: [[0, 0]] });
  await load;
  const [world, a, b] = workers;
  const asked = [client.draw(0, 0), client.draw(1, 0), client.draw(2, 0), client.patch(new Uint8Array([1]))];
  answer(world, { bytes: new ArrayBuffer(0) }, world.sent[1]);
  await tick();
  answer(a, { error: 'The mesher crashed (unreachable)', broken: true });
  for (const request of asked) await assert.rejects(request, /The mesher crashed \(unreachable\)/);
  assert.ok(workers.every((worker) => worker.ended), 'every worker of the pool is ended, with its memory');
  const sent = workers.reduce((n, worker) => n + worker.sent.length, 0);
  await assert.rejects(client.patch(new Uint8Array([1])), /The mesher crashed/);
  await assert.rejects(client.draw(0, 0), /The mesher crashed/);
  assert.equal(workers.reduce((n, worker) => n + worker.sent.length, 0), sent, 'nothing more reaches the crashed pool');
  answer(b, { bytes: new ArrayBuffer(0) });
  load = client.load(FAKE_VIEW, false);
  assert.equal(workers.length, 6, 'the next load starts a fresh pool');
  answer(workers[3], { tiles: [[0, 0]], changed: [[0, 0]] });
  assert.deepEqual((await load).tiles, [[0, 0]]);
});

// A blueprint whose min x lies past its max x: the module crashes on it (a
// WebAssembly trap: its volume has a negative size).
function inverted(bytes) {
  const out = Buffer.from(bytes);
  let at = 4;
  const skip = () => { at += 2 + out.readUInt16BE(at); };
  for (let entries = out.readInt32BE(0); entries > 0; entries -= 1) {
    skip();
    for (let n = out.readInt32BE(at), i = (at += 4, 0); i < n; i += 1) { skip(); skip(); }
  }
  out.writeInt32BE(out.readInt32BE(at + 12) + 100, at);
  return out;
}

test('a world the module crashes on ends the workers, and the next world loads in fresh ones', async (t) => {
  const spawned = [];
  const client = await nodeWorker(t, 2, spawned);
  const blocks = sampleBlocks();
  const mask = renderMask(bounds(blocks));
  const bytes = blueprint(blocks);
  const view = { wasm: dataUrl(WASM), pack: dataUrl(PACK), blueprint: dataUrl(bytes), mask };
  await client.load(view, false);
  await assert.rejects(client.load({ ...view, blueprint: dataUrl(inverted(bytes)) }, true), /^Error: The mesher crashed \(unreachable\)$/);
  await assert.rejects(client.draw(0, 0), /The mesher crashed/);
  for (let tries = 0; spawned.some((worker) => !worker.ended) && tries < 100; tries += 1) await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(spawned.map((worker) => worker.ended), [true, true], 'the crashed workers are ended');
  const { Mesher } = await import('../web/mesher.mjs');
  const direct = await Mesher.create(WASM, PACK);
  const { tiles } = direct.load(bytes, mask, false);
  const again = await client.load(view, true);
  assert.equal(spawned.length, 4, 'fresh workers');
  assert.deepEqual(again.changed, tiles, 'fresh workers hold no world before: every tile is drawn');
  for (const [x, z] of tiles) assert.ok(Buffer.from(await client.draw(x, z)).equals(Buffer.from(direct.drawIndexed(x, z))), `tile ${x},${z}`);
});
