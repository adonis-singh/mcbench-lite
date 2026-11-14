'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// The viewer's world as the light reads it: chunks ('cx,cy,cz' -> Map of
// placed blocks by local index), light sources, and the opaque cells the
// relaxation reference reads.
function fixture() {
  const chunks = new Map();
  const world = {
    chunks,
    opaqueCells: new Set(),
    emitters: new Map(),
    size: 0,
    forEachBlock(fn) { for (const chunk of chunks.values()) for (const placed of chunk.values()) fn(placed); },
  };
  const touched = new Set();
  const chunkKey = (x, y, z) => `${x >> 4},${y >> 4},${z >> 4}`;
  const local = (x, y, z) => ((y & 15) * 16 + (z & 15)) * 16 + (x & 15);
  const remove = (x, y, z) => {
    const key = chunkKey(x, y, z);
    const chunk = chunks.get(key);
    const position = `${x},${y},${z}`;
    touched.add(key);
    if (!chunk?.has(local(x, y, z))) return;
    chunk.delete(local(x, y, z));
    world.size -= 1;
    world.opaqueCells.delete(position);
    world.emitters.delete(position);
  };
  const put = (x, y, z, opaque, emission = 0) => {
    remove(x, y, z);
    const key = chunkKey(x, y, z);
    if (!chunks.has(key)) chunks.set(key, new Map());
    chunks.get(key).set(local(x, y, z), { pos: [x, y, z], opaque, lum: emission });
    world.size += 1;
    const position = `${x},${y},${z}`;
    if (opaque) world.opaqueCells.add(position);
    if (emission) world.emitters.set(position, emission);
  };
  const take = () => { const keys = [...touched]; touched.clear(); return keys; };
  return { world, put, remove, take };
}

// Independent reference: repeatedly relax every air cell until the light
// field converges, instead of using the renderer's work queue.
function reference(world, grid) {
  const levels = new Map();
  const key = (x, y, z) => `${x},${y},${z}`;
  for (let x = grid.minX; x <= grid.maxX; x++) for (let z = grid.minZ; z <= grid.maxZ; z++) {
    for (let y = grid.maxY; y >= grid.minY; y--) {
      if (world.opaqueCells.has(key(x, y, z))) break;
      levels.set(key(x, y, z), 15);
    }
  }
  for (const [position, emission] of world.emitters) levels.set(position, Math.max(levels.get(position) || 0, emission));
  let changed;
  do {
    changed = false;
    for (let x = grid.minX; x <= grid.maxX; x++) for (let y = grid.minY; y <= grid.maxY; y++) for (let z = grid.minZ; z <= grid.maxZ; z++) {
      const position = key(x, y, z);
      if (world.opaqueCells.has(position)) continue;
      const old = levels.get(position) || 0;
      const next = Math.max(old, ...[
        [x + 1, y, z], [x - 1, y, z], [x, y + 1, z], [x, y - 1, z], [x, y, z + 1], [x, y, z - 1],
      ].map((p) => (levels.get(key(...p)) || 0) - 1));
      if (next > old) { levels.set(position, next); changed = true; }
    }
  } while (changed);
  return levels;
}

// The viewer's light before it was kept between updates: the whole world lit
// from nothing on every update, kept here as it was, as the definition the
// incremental light must equal byte for byte.
  function fullLight(world) {
    if (world.size === 0) return null;
    let minX = Infinity, minY = Infinity, minZ = Infinity;
    let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
    world.forEachBlock(({ pos }) => {
      if (pos[0] < minX) minX = pos[0]; if (pos[0] > maxX) maxX = pos[0];
      if (pos[1] < minY) minY = pos[1]; if (pos[1] > maxY) maxY = pos[1];
      if (pos[2] < minZ) minZ = pos[2]; if (pos[2] > maxZ) maxZ = pos[2];
    });
    minX -= 1; minY -= 1; minZ -= 1; maxX += 1; maxY += 1; maxZ += 1;
    const sx = maxX - minX + 1, sy = maxY - minY + 1, sz = maxZ - minZ + 1;
    if (sx * sy * sz > 16_000_000) return null;
    const vol = sx * sy * sz;
    const at = (x, y, z) => ((y - minY) * sz + (z - minZ)) * sx + (x - minX);
    const solid = new Uint8Array(vol);
    world.forEachBlock(({ pos, opaque }) => {
      if (opaque) solid[at(pos[0], pos[1], pos[2])] = 1;
    });
    const lvl = new Uint8Array(vol);
    // At most one pending entry per cell. Queue indexes replace coordinate
    // triples and per-visit neighbor arrays; memory stays bounded by the grid.
    const queue = new Uint32Array(vol);
    const queued = new Uint8Array(vol);
    let head = 0; let tail = 0; let pending = 0;
    const enqueue = (index) => {
      if (queued[index]) return;
      queued[index] = 1;
      queue[tail] = index;
      tail = (tail + 1) % vol;
      pending += 1;
    };
    for (let x = minX; x <= maxX; x++) {
      for (let z = minZ; z <= maxZ; z++) {
        for (let y = maxY; y >= minY; y--) {
          const i = at(x, y, z);
          if (solid[i]) break;
          lvl[i] = 15;
          enqueue(i);
        }
      }
    }
    for (const [key, lum] of world.emitters) {
      const [x, y, z] = key.split(',').map(Number);
      const i = at(x, y, z);
      if (lum > lvl[i]) {
        lvl[i] = lum;
        enqueue(i);
      }
    }
    const spread = (index, level) => {
      if (solid[index] || lvl[index] >= level) return;
      lvl[index] = level;
      enqueue(index);
    };
    const plane = sx * sz;
    while (pending > 0) {
      const index = queue[head];
      head = (head + 1) % vol;
      pending -= 1;
      queued[index] = 0;
      const next = lvl[index] - 1;
      if (next <= 0) continue;
      const x = index % sx;
      const z = Math.floor(index / sx) % sz;
      if (x + 1 < sx) spread(index + 1, next);
      if (x > 0) spread(index - 1, next);
      if (index + plane < vol) spread(index + plane, next);
      if (index >= plane) spread(index - plane, next);
      if (z + 1 < sz) spread(index + sx, next);
      if (z > 0) spread(index - sx, next);
    }
    return { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, lvl, at };
  }


function sameField(actual, expected, label) {
  if (!expected) return assert.equal(actual, null, label);
  for (const name of ['minX', 'minY', 'minZ', 'maxX', 'maxY', 'maxZ', 'sx', 'sz']) assert.equal(actual?.[name], expected[name], `${label}: ${name}`);
  assert.ok(Buffer.from(actual.lvl).equals(Buffer.from(expected.lvl)), `${label}: light arrays differ`);
}

test('voxel lighting preserves skylight, shadows, and competing emitters', async () => {
  const { computeLight } = await import('../web/light.mjs');
  for (let seed = 1; seed <= 8; seed++) {
    const { world, put } = fixture();
    for (let x = -5; x <= 5; x++) for (let z = -5; z <= 5; z++) {
      put(x, -3, z, true);
      if ((x * x + z * z + seed) % 7 !== 0) put(x, 3, z, true);
      if ((x * 3 + z + seed) % 4 === 0) put(x, 0, z, true);
    }
    put(-3, -1, -3, false, seed);
    put(3, -1, 3, true, 15);
    const actual = computeLight(world);
    const expected = reference(world, actual);
    for (let x = actual.minX; x <= actual.maxX; x++) for (let y = actual.minY; y <= actual.maxY; y++) for (let z = actual.minZ; z <= actual.maxZ; z++) {
      assert.equal(actual.lvl[actual.at(x, y, z)], expected.get(`${x},${y},${z}`) || 0, `seed ${seed}: ${x},${y},${z}`);
    }
    sameField(actual, fullLight(world), `seed ${seed}`);
  }
});

test('empty worlds and extents above the existing light cap retain direct skylight', async () => {
  const { computeLight } = await import('../web/light.mjs');
  const { world, put } = fixture();
  assert.equal(computeLight(world), null);
  put(-1000, -1000, -1000, true);
  put(1000, 1000, 1000, true);
  assert.equal(computeLight(world), null);
});

test('the chunks whose light changed: skylight down a column whose roof went, and direct skylight past the cap', async () => {
  const { computeLight, changedLightChunks } = await import('../web/light.mjs');
  const roofed = fixture();
  const open = fixture();
  for (let x = 0; x <= 2; x++) for (let z = 0; z <= 2; z++) {
    roofed.put(x, 0, z, true);
    roofed.put(x, 40, z, true);
    open.put(x, 0, z, true);
  }
  const before = computeLight(roofed.world);
  const after = computeLight(open.world);
  assert.deepEqual(changedLightChunks(before, before), new Set());
  assert.deepEqual(changedLightChunks(null, null), new Set());
  // Under the roof, 13 at the column's middle, now 15: in chunks with no block changed.
  const changed = changedLightChunks(before, after);
  assert.ok(changed.has('0,1,0'), [...changed].join(' '));
  assert.ok(changed.has('0,2,0'));
  assert.ok(!changed.has('0,3,0'), 'above the roof nothing changed');
  // A field gone past the cap is direct skylight: every darker cell changed,
  // under the floor too.
  assert.deepEqual(changedLightChunks(before, null), new Set(['0,-1,0', ...changed]));
});

// A seeded generator (mulberry32), so a failure names the seed that made it.
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// After every batch of edits the light kept between updates equals the
// whole world lit from nothing, byte for byte, and the chunks it says
// changed are the chunks whose light differs between the two fields.
test('the light kept between edits equals the whole world lit from nothing after random edits', async () => {
  const { createLight, updateLight, changedLightChunks } = await import('../web/light.mjs');
  let batches = 0;
  const seeds = Number(process.env.LIGHT_SEEDS || 60);
  for (let seed = 1; seed <= seeds; seed++) {
    const rand = random(seed);
    const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
    const { world, put, remove, take } = fixture();
    const light = createLight();
    // A region across chunk borders on every axis, negative coordinates included.
    const span = { x: [int(-20, -2), int(2, 24)], y: [int(-20, -1), int(1, 20)], z: [int(-20, -2), int(2, 24)] };
    const anywhere = () => [int(...span.x), int(...span.y), int(...span.z)];
    const block = (x, y, z) => {
      const kind = rand();
      if (kind < 0.55) put(x, y, z, true);
      else if (kind < 0.8) put(x, y, z, false);
      else if (kind < 0.9) put(x, y, z, false, int(1, 15));
      else put(x, y, z, true, int(1, 15));
    };
    for (let n = int(0, 300); n > 0; n--) block(...anywhere());
    let previous = fullLight(world);
    sameField((updateLight(light, world, take()), light.field), previous, `seed ${seed} start`);
    for (let round = 0; round < 25; round++) {
      const kind = rand();
      if (kind < 0.25) {
        // A few blocks anywhere, put or taken away.
        for (let n = int(1, 6); n > 0; n--) (rand() < 0.5 ? remove(...anywhere()) : block(...anywhere()));
      } else if (kind < 0.4) {
        // A roof over a patch, or the roof taken away: the sky closes and opens.
        const [x0, , z0] = anywhere();
        const y = int(span.y[0], span.y[1] + 10);
        const roof = rand() < 0.5;
        for (let x = x0; x < x0 + int(1, 6); x++) for (let z = z0; z < z0 + int(1, 6); z++) (roof ? put(x, y, z, true) : remove(x, y, z));
      } else if (kind < 0.55) {
        // An emitter put, moved or put out.
        const at = anywhere();
        if (rand() < 0.5) put(...at, rand() < 0.5, int(1, 15)); else remove(...at);
      } else if (kind < 0.7) {
        // At a chunk border.
        const [x, y, z] = anywhere().map((v) => (v & ~15) + (rand() < 0.5 ? 0 : -1));
        for (let n = int(1, 4); n > 0; n--) block(x + int(0, 1) * (rand() < 0.5 ? 1 : 0), y, z);
      } else if (kind < 0.85) {
        // Past the bounds (the box grows), or the blocks at a bound taken away (it shrinks).
        if (rand() < 0.5) {
          const axis = int(0, 2);
          const at = anywhere();
          at[axis] += (rand() < 0.5 ? -1 : 1) * int(1, 20);
          block(...at);
        } else {
          const blocks = [];
          world.forEachBlock(({ pos }) => blocks.push(pos));
          if (blocks.length) {
            const axis = int(0, 2);
            const extreme = rand() < 0.5 ? Math.min(...blocks.map((p) => p[axis])) : Math.max(...blocks.map((p) => p[axis]));
            for (const pos of blocks) if (pos[axis] === extreme) remove(...pos);
          }
        }
      } else if (kind < 0.9) {
        // Everything taken away (no light), then some put back.
        const blocks = [];
        world.forEachBlock(({ pos }) => blocks.push(pos));
        for (const pos of blocks) if (rand() < 0.97) remove(...pos);
      } else if (kind < 0.95) {
        // A block far off makes the box pass the cap (no field), and then goes.
        if (rand() < 0.5) put(span.x[0] - 400, span.y[0] - 120, span.z[0] - 400, true);
        else remove(span.x[0] - 400, span.y[0] - 120, span.z[0] - 400);
      } else {
        // A column of blocks: sky over its whole height.
        const [x, , z] = anywhere();
        for (let y = span.y[0]; y <= span.y[1]; y++) if (rand() < 0.3) block(x, y, z);
      }
      const expected = fullLight(world);
      const changed = updateLight(light, world, take());
      sameField(light.field, expected, `seed ${seed} round ${round}`);
      assert.deepEqual(changed, changedLightChunks(previous, expected), `seed ${seed} round ${round}: changed chunks`);
      previous = expected;
      batches += 1;
    }
  }
  assert.equal(batches, seeds * 25);
});
