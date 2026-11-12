'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// The viewer's world as computeLight reads it, plus the opaque cells the
// reference reads.
function fixture() {
  const map = new Map();
  const world = {
    opaqueCells: new Set(),
    emitters: new Map(),
    get size() { return map.size; },
    forEachBlock(fn) { for (const placed of map.values()) fn(placed); },
  };
  const put = (x, y, z, opaque, emission = 0) => {
    const key = `${x},${y},${z}`;
    map.set(key, { pos: [x, y, z], opaque });
    if (opaque) world.opaqueCells.add(key);
    if (emission) world.emitters.set(key, emission);
  };
  return { world, put };
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
