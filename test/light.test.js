'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

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

// Face shapes as the quarters of a face they cover (bits 0-3): shape n
// covers the quarters of n, so 0 is the empty face and 15 the full one, and
// two faces close to light when together they cover all four.
const SHAPES = 16;
const OCCLUDES = Uint8Array.from({ length: SHAPES * SHAPES }, (_, at) => Number(((at / SHAPES | 0) | at % SHAPES) === 15));

// Block states as Minecraft's table gives them, by state id (0 air): a stone
// that blocks all light, glass, water and leaves that dim it by one, a slab
// whose bottom face is full and whose sides are half faces, the top slab
// beside it (whose half faces close the face between them), stairs, light
// blocks of every level, emitters that block light, and random states.
function states(rand) {
  const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
  const list = [
    { emission: 0, lightBlock: 0 },
    { emission: 0, lightBlock: 15 },
    { emission: 0, lightBlock: 0 },
    { emission: 0, lightBlock: 1 },
    { emission: 0, lightBlock: 1 },
    { emission: 0, lightBlock: 0, faces: [15, 0, 3, 3, 3, 3] },
    { emission: 0, lightBlock: 0, faces: [0, 15, 12, 12, 12, 12] },
    { emission: 0, lightBlock: 0, faces: [15, 3, 15, 3, 5, 10] },
    { emission: 15, lightBlock: 15 },
    { emission: 13, lightBlock: 15, faces: [15, 15, 15, 15, 15, 15] },
  ];
  for (let level = 1; level <= 15; level++) list.push({ emission: level, lightBlock: 0 });
  for (let n = 0; n < 8; n++) {
    list.push({
      emission: rand() < 0.3 ? int(1, 15) : 0, lightBlock: [0, 1, 2, int(3, 14), 15][int(0, 4)],
      faces: rand() < 0.5 ? Array.from({ length: 6 }, () => [0, 15, int(1, 14)][int(0, 2)]) : undefined,
    });
  }
  return list;
}
// Sky light falls straight down through a state that blocks no light (as
// Paper's light engine tests it).
const skyPasses = (list, state) => list[state].lightBlock === 0;

// The light rule (web/light.mjs lightRule) of `list`.
async function ruleOf(list) {
  const { lightRule } = await import('../web/light.mjs');
  const faces = new Uint8Array(list.length * 6);
  list.forEach((state, id) => state.faces && faces.set(state.faces, id * 6));
  return lightRule({
    emission: Uint8Array.from(list, (s) => s.emission),
    lightBlock: Uint8Array.from(list, (s) => s.lightBlock),
    faces,
    shapes: SHAPES,
    occludes: OCCLUDES,
  });
}

// The viewer's world as the light reads it: chunks ('cx,cy,cz' -> Map of
// placed blocks by local index, each with its light class), and each cell's
// state, which the references read.
function fixture(rule) {
  const chunks = new Map();
  const cells = new Map(); // 'x,y,z' -> state id
  const world = {
    chunks,
    cells,
    size: 0,
    forEachBlock(fn) { for (const chunk of chunks.values()) for (const placed of chunk.values()) fn(placed); },
  };
  const touched = new Set();
  const chunkKey = (x, y, z) => `${x >> 4},${y >> 4},${z >> 4}`;
  const local = (x, y, z) => ((y & 15) * 16 + (z & 15)) * 16 + (x & 15);
  const remove = (x, y, z) => {
    const key = chunkKey(x, y, z);
    const chunk = chunks.get(key);
    touched.add(key);
    if (!chunk?.has(local(x, y, z))) return;
    chunk.delete(local(x, y, z));
    world.size -= 1;
    cells.delete(`${x},${y},${z}`);
  };
  const put = (x, y, z, state) => {
    remove(x, y, z);
    const key = chunkKey(x, y, z);
    if (!chunks.has(key)) chunks.set(key, new Map());
    chunks.get(key).set(local(x, y, z), { pos: [x, y, z], light: rule.classOf[state] });
    world.size += 1;
    cells.set(`${x},${y},${z}`, state);
  };
  const take = () => { const keys = [...touched]; touched.clear(); return keys; };
  return { world, put, remove, take };
}

// A world's floor far below every block of a test that does not reach it.
const DEEP = -100000;

const STEPS = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
const facesOf = (list, state) => list[state].faces || [0, 0, 0, 0, 0, 0];
// Whether light passes from a cell of state `a` into its neighbour of state
// `b` in direction `d`: the faces they turn to each other do not close.
const opens = (list, a, b, d) => !OCCLUDES[facesOf(list, a)[d] * SHAPES + facesOf(list, b)[d ^ 1]];

// Independent reference: repeatedly relax every cell until the light field
// converges, from each state's facts, instead of the renderer's work queue.
function reference(list, world, grid) {
  const levels = new Map();
  const key = (x, y, z) => `${x},${y},${z}`;
  const stateAt = (x, y, z) => world.cells.get(key(x, y, z)) ?? 0;
  for (let x = grid.minX; x <= grid.maxX; x++) for (let z = grid.minZ; z <= grid.maxZ; z++) {
    let above = 0;
    for (let y = grid.maxY; y >= grid.minY; y--) {
      const state = stateAt(x, y, z);
      if (!skyPasses(list, state) || !opens(list, above, state, 0)) break;
      levels.set(key(x, y, z), 15);
      above = state;
    }
  }
  for (const [position, state] of world.cells) levels.set(position, Math.max(levels.get(position) || 0, list[state].emission));
  let changed;
  do {
    changed = false;
    for (let x = grid.minX; x <= grid.maxX; x++) for (let y = grid.minY; y <= grid.maxY; y++) for (let z = grid.minZ; z <= grid.maxZ; z++) {
      const position = key(x, y, z);
      const state = stateAt(x, y, z);
      const old = levels.get(position) || 0;
      let next = old;
      STEPS.forEach(([dx, dy, dz], d) => {
        const [nx, ny, nz] = [x - dx, y - dy, z - dz];
        if (nx < grid.minX || nx > grid.maxX || ny < grid.minY || ny > grid.maxY || nz < grid.minZ || nz > grid.maxZ) return;
        if (!opens(list, stateAt(nx, ny, nz), state, d)) return;
        next = Math.max(next, (levels.get(key(nx, ny, nz)) || 0) - Math.max(1, list[state].lightBlock));
      });
      if (next > old) { levels.set(position, next); changed = true; }
    }
  } while (changed);
  return levels;
}

// The whole world lit from nothing with one work queue, as the viewer lit
// it on every update before the light was kept between updates (that code,
// with Minecraft's rule for each cell's state in place of opaque or not,
// and no cell below the world's floor): the definition the incremental
// light must equal byte for byte.
function fullLight(list, world, floor) {
  if (world.size === 0) return null;
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  world.forEachBlock(({ pos }) => {
    if (pos[0] < minX) minX = pos[0]; if (pos[0] > maxX) maxX = pos[0];
    if (pos[1] < minY) minY = pos[1]; if (pos[1] > maxY) maxY = pos[1];
    if (pos[2] < minZ) minZ = pos[2]; if (pos[2] > maxZ) maxZ = pos[2];
  });
  minX -= 1; minY = Math.max(minY - 1, floor); minZ -= 1; maxX += 1; maxY += 1; maxZ += 1;
  const sx = maxX - minX + 1, sy = maxY - minY + 1, sz = maxZ - minZ + 1;
  if (sx * sy * sz > 16_000_000) return null;
  const vol = sx * sy * sz;
  const at = (x, y, z) => ((y - minY) * sz + (z - minZ)) * sx + (x - minX);
  const state = new Uint16Array(vol);
  for (const [position, id] of world.cells) state[at(...position.split(',').map(Number))] = id;
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
      let above = 0;
      for (let y = maxY; y >= minY; y--) {
        const i = at(x, y, z);
        if (!skyPasses(list, state[i]) || !opens(list, above, state[i], 0)) break;
        lvl[i] = 15;
        enqueue(i);
        above = state[i];
      }
    }
  }
  for (const [position, id] of world.cells) {
    const i = at(...position.split(',').map(Number));
    if (list[id].emission > lvl[i]) {
      lvl[i] = list[id].emission;
      enqueue(i);
    }
  }
  const spread = (from, index, d) => {
    if (!opens(list, state[from], state[index], d)) return;
    const level = lvl[from] - Math.max(1, list[state[index]].lightBlock);
    if (lvl[index] >= level) return;
    lvl[index] = level;
    enqueue(index);
  };
  const plane = sx * sz;
  while (pending > 0) {
    const index = queue[head];
    head = (head + 1) % vol;
    pending -= 1;
    queued[index] = 0;
    if (lvl[index] <= 1) continue;
    const x = index % sx;
    const z = Math.floor(index / sx) % sz;
    if (x + 1 < sx) spread(index, index + 1, 5);
    if (x > 0) spread(index, index - 1, 4);
    if (index + plane < vol) spread(index, index + plane, 1);
    if (index >= plane) spread(index, index - plane, 0);
    if (z + 1 < sz) spread(index, index + sx, 3);
    if (z > 0) spread(index, index - sx, 2);
  }
  return { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, lvl, at };
}

function sameField(actual, expected, label) {
  if (!expected) return assert.equal(actual, null, label);
  for (const name of ['minX', 'minY', 'minZ', 'maxX', 'maxY', 'maxZ', 'sx', 'sz']) assert.equal(actual?.[name], expected[name], `${label}: ${name}`);
  assert.ok(Buffer.from(actual.lvl).equals(Buffer.from(expected.lvl)), `${label}: light arrays differ`);
}

test('voxel lighting preserves skylight, shadows, dimming, face shapes and competing emitters', async () => {
  const { computeLight } = await import('../web/light.mjs');
  for (let seed = 1; seed <= 8; seed++) {
    const rand = random(seed);
    const list = states(rand);
    const rule = await ruleOf(list);
    const { world, put } = fixture(rule);
    const any = () => 1 + Math.floor(rand() * (list.length - 1));
    for (let x = -5; x <= 5; x++) for (let z = -5; z <= 5; z++) {
      put(x, -3, z, 1);
      if ((x * x + z * z + seed) % 7 !== 0) put(x, 3, z, rand() < 0.7 ? 1 : any());
      if ((x * 3 + z + seed) % 4 === 0) put(x, 0, z, any());
      if (rand() < 0.1) put(x, -1, z, any());
    }
    put(-3, -1, -3, 10 + seed);
    put(3, -1, 3, 8);
    // Every other world stands on the floor: no cell lies below its lowest blocks.
    const floor = seed % 2 ? DEEP : -3;
    const actual = computeLight(rule, floor, world);
    assert.equal(actual.minY, seed % 2 ? -4 : -3);
    const expected = reference(list, world, actual);
    for (let x = actual.minX; x <= actual.maxX; x++) for (let y = actual.minY; y <= actual.maxY; y++) for (let z = actual.minZ; z <= actual.maxZ; z++) {
      assert.equal(actual.lvl[actual.at(x, y, z)], expected.get(`${x},${y},${z}`) || 0, `seed ${seed}: ${x},${y},${z}`);
    }
    sameField(actual, fullLight(list, world, floor), `seed ${seed}`);
  }
});

test('two half faces that cover a face together close it; light steps into a cell less its light block', async () => {
  const { computeLight } = await import('../web/light.mjs');
  const list = states(random(1));
  const rule = await ruleOf(list);
  // A tunnel under a stone roof, open at x = -1: a bottom slab beside a top
  // slab passes no light along it, two bottom slabs do (half faces).
  const tunnel = (second) => {
    const { world, put } = fixture(rule);
    for (let x = 0; x <= 6; x++) {
      for (const [y, z] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) put(x, y, z, 1);
      put(x, 0, 0, 0);
    }
    put(7, 0, 0, 1);
    put(2, 0, 0, 5);
    put(3, 0, 0, second);
    for (let x = 0; x <= 6; x++) for (let y = -1; y <= 1; y++) put(x, y, 2, 1), put(x, y, -2, 1);
    for (let x = 0; x <= 7; x++) for (let z = -2; z <= 2; z++) put(x, 2, z, 1);
    return computeLight(rule, DEEP, world);
  };
  const closed = tunnel(6);
  const open = tunnel(5);
  // From the open sky at x = -1 (15): 14, 13 into the first slab, then 12 into the second where it passes.
  assert.deepEqual([0, 1, 2, 3, 4].map((x) => closed.lvl[closed.at(x, 0, 0)]), [14, 13, 12, 0, 0]);
  assert.deepEqual([0, 1, 2, 3, 4].map((x) => open.lvl[open.at(x, 0, 0)]), [14, 13, 12, 11, 10]);
  // A shaft in stone under glass holds the sky all the way down; under water
  // the sky stops at the water, which takes 15 less its light block, and
  // each cell below one less.
  const shaft = (top) => {
    const { world, put } = fixture(rule);
    for (let y = 0; y <= 5; y++) for (let x = -1; x <= 1; x++) for (let z = -1; z <= 1; z++) put(x, y, z, x === 0 && z === 0 ? 0 : 1);
    put(0, 5, 0, top);
    put(0, -1, 0, 1);
    const lit = computeLight(rule, DEEP, world);
    return [5, 4, 3, 2].map((y) => lit.lvl[lit.at(0, y, 0)]);
  };
  assert.deepEqual(shaft(2), [15, 15, 15, 15]);
  assert.deepEqual(shaft(3), [14, 13, 12, 11]);
  // A cell walled in on every side but below gets light only from below:
  // none at the world's floor, where there is no below.
  const pocket = (floor) => {
    const { world, put } = fixture(rule);
    for (const [x, y, z] of [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0]]) put(x, y, z, 1);
    const lit = computeLight(rule, floor, world);
    return lit.lvl[lit.at(0, 0, 0)];
  };
  assert.equal(pocket(0), 0);
  assert.equal(pocket(-1), 12);
});

test('empty worlds and extents above the existing light cap retain direct skylight', async () => {
  const { computeLight } = await import('../web/light.mjs');
  const rule = await ruleOf(states(random(1)));
  const { world, put } = fixture(rule);
  assert.equal(computeLight(rule, DEEP, world), null);
  put(-1000, -1000, -1000, 1);
  put(1000, 1000, 1000, 1);
  assert.equal(computeLight(rule, DEEP, world), null);
});

test('the chunks whose light changed: skylight down a column whose roof went, and direct skylight past the cap', async () => {
  const { computeLight, changedLightChunks } = await import('../web/light.mjs');
  const rule = await ruleOf(states(random(1)));
  const roofed = fixture(rule);
  const open = fixture(rule);
  for (let x = 0; x <= 2; x++) for (let z = 0; z <= 2; z++) {
    roofed.put(x, 0, z, 1);
    roofed.put(x, 40, z, 1);
    open.put(x, 0, z, 1);
  }
  const before = computeLight(rule, DEEP, roofed.world);
  const after = computeLight(rule, DEEP, open.world);
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
    const list = states(rand);
    const rule = await ruleOf(list);
    const fixed = fixture(rule);
    const { world, remove, take } = fixed;
    const any = () => int(1, list.length - 1);
    // A region across chunk borders on every axis, negative coordinates included.
    const span = { x: [int(-20, -2), int(2, 24)], y: [int(-20, -1), int(1, 20)], z: [int(-20, -2), int(2, 24)] };
    // Half the worlds have their floor at the region's bottom (or a cell
    // below it), where no block is put and no light is held below.
    const floor = rand() < 0.5 ? span.y[0] - int(0, 1) : DEEP;
    const put = (x, y, z, state) => y >= floor && fixed.put(x, y, z, state);
    const light = createLight(rule, floor);
    const anywhere = () => [int(...span.x), int(...span.y), int(...span.z)];
    // Mostly stone, the rest any state: emitters, dimmers, glass, slabs and stairs.
    const block = (x, y, z) => put(x, y, z, rand() < 0.45 ? 1 : any());
    const roofs = [1, 1, 3, 4, 5, 6, 7];
    for (let n = int(0, 300); n > 0; n--) block(...anywhere());
    let previous = fullLight(list, world, floor);
    sameField((updateLight(light, world, take()), light.field), previous, `seed ${seed} start`);
    for (let round = 0; round < 25; round++) {
      const kind = rand();
      if (kind < 0.25) {
        // A few blocks anywhere, put or taken away.
        for (let n = int(1, 6); n > 0; n--) (rand() < 0.5 ? remove(...anywhere()) : block(...anywhere()));
      } else if (kind < 0.4) {
        // A roof over a patch (stone, water, leaves, slabs or stairs), or the
        // roof taken away: the sky closes and opens.
        const [x0, , z0] = anywhere();
        const y = int(span.y[0], span.y[1] + 10);
        const roof = rand() < 0.5;
        const state = roofs[int(0, roofs.length - 1)];
        for (let x = x0; x < x0 + int(1, 6); x++) for (let z = z0; z < z0 + int(1, 6); z++) (roof ? put(x, y, z, state) : remove(x, y, z));
      } else if (kind < 0.55) {
        // An emitter put, moved or put out: a light block of any level, or one that blocks light.
        const at = anywhere();
        if (rand() < 0.5) put(...at, rand() < 0.7 ? 9 + int(1, 15) : 8); else remove(...at);
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
        if (rand() < 0.5) put(span.x[0] - 4500, span.y[0], span.z[0] - 4500, 1);
        else remove(span.x[0] - 4500, span.y[0], span.z[0] - 4500);
      } else {
        // A column of blocks: sky over its whole height.
        const [x, , z] = anywhere();
        for (let y = span.y[0]; y <= span.y[1]; y++) if (rand() < 0.3) block(x, y, z);
      }
      const expected = fullLight(list, world, floor);
      const changed = updateLight(light, world, take());
      sameField(light.field, expected, `seed ${seed} round ${round}`);
      assert.deepEqual(changed, changedLightChunks(previous, expected), `seed ${seed} round ${round}: changed chunks`);
      previous = expected;
      batches += 1;
    }
  }
  assert.equal(batches, seeds * 25);
});
