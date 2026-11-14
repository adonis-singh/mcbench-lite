'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');

const { MODULE } = require('../scripts/build-renderer');
const { blocksBlueprint } = require('../server/blueprint');
const { lightPack } = require('./helpers/mesher-pack');

// see_build's light (web/light.mjs) against the live viewer's mesher
// (renderer/, the committed WebAssembly module the live viewer runs), whose
// light is checked against scripts/McbenchBlueMapReference.java by
// scripts/mesher-parity.js and cell for cell against its own light lit from
// nothing in its tests (renderer/src/world.rs). Both light a build standing
// alone in open sky, above the world's floor (y = -64 here, the mesher's);
// the mesher keeps sky and block light apart, the page the brighter of the
// two. Each state's facts are drawn at random, as a block table gives them:
// sky light passes straight down through a state that blocks no light (the
// page tests the light block, as Paper's light engine does, the mesher
// propagatesSkylightDown, and the two agree for every state of the pinned
// Minecraft). The mesher closes a face to light where one side of it is a
// full face shape; the page tests the pair (Shapes.faceShapeOccludes), and
// two partial faces that cover a face together close it to the page's
// light, not the mesher's: these states' partial faces never cover a face
// together, where the two rules are one (test/light.test.js covers the
// pairs that do).

const WASM = fs.readFileSync(MODULE);
const MASK = { minX: -100000, maxX: 100000, minZ: -100000, maxZ: 100000 };
const FLOOR = -64;
// Face shapes: 0 empty, 1 full, 2 and 3 partial (a slab's half side, a
// stair's step), which close a face only beside a full one.
const SHAPES = 4;
const OCCLUDES = Uint8Array.from({ length: SHAPES * SHAPES }, (_, at) => Number((at / SHAPES | 0) === 1 || at % SHAPES === 1));

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

// Block states lit as Minecraft's are (emission, light block, and for a
// state that is conditionally full opaque its six face shapes, down, up,
// north, south, west, east), and as many more made at random.
function kinds(rand) {
  const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
  const states = [
    { name: 'air', emission: 0, lightBlock: 0 },
    { name: 'stone', emission: 0, lightBlock: 15 },
    { name: 'glass', emission: 0, lightBlock: 0 },
    { name: 'water', emission: 0, lightBlock: 1 },
    { name: 'leaves', emission: 0, lightBlock: 1 },
    { name: 'wall', emission: 0, lightBlock: 0 },
    { name: 'glowstone', emission: 15, lightBlock: 15 },
    { name: 'torch', emission: 14, lightBlock: 0 },
    { name: 'bottom slab', emission: 0, lightBlock: 0, faces: [1, 0, 2, 2, 2, 2] },
    { name: 'top slab', emission: 0, lightBlock: 0, faces: [0, 1, 2, 2, 2, 2] },
    { name: 'waterlogged slab', emission: 0, lightBlock: 1, faces: [1, 0, 2, 2, 2, 2] },
    { name: 'stairs', emission: 0, lightBlock: 0, faces: [1, 2, 1, 3, 3, 3] },
    { name: 'upside-down stairs', emission: 0, lightBlock: 0, faces: [2, 1, 3, 3, 3, 1] },
    { name: 'double slab', emission: 0, lightBlock: 15, faces: [1, 1, 1, 1, 1, 1] },
  ];
  for (let level = 1; level <= 15; level++) states.push({ name: `light[level=${level}]`, emission: level, lightBlock: 0 });
  for (let n = 0; n < 12; n++) {
    states.push({
      name: `random ${n}`, emission: rand() < 0.4 ? int(1, 15) : 0, lightBlock: [0, 0, 1, 1, 2, int(3, 14), 15][int(0, 6)],
      faces: rand() < 0.5 ? Array.from({ length: 6 }, () => int(0, 3)) : undefined,
    });
  }
  return states;
}

// The page's rule for `states` and the mesher's pack of them (light flags:
// 1 sky light passes down, 2 shapes light with its faces; `full`, the faces
// whose shape is full).
async function rules(states) {
  const { lightRule } = await import('../web/light.mjs');
  const faces = new Uint8Array(states.length * 6);
  states.forEach((state, id) => state.faces && faces.set(state.faces, id * 6));
  const rule = lightRule({
    emission: Uint8Array.from(states, (s) => s.emission),
    lightBlock: Uint8Array.from(states, (s) => s.lightBlock),
    faces,
    shapes: SHAPES,
    occludes: OCCLUDES,
  });
  const pack = lightPack(states.map((s) => ({
    emission: s.emission,
    lightBlock: s.lightBlock,
    lightFlags: (s.lightBlock === 0 ? 1 : 0) | (s.faces ? 2 : 0),
    full: s.faces ? s.faces.reduce((mask, shape, d) => mask | (shape === 1 ? 1 << d : 0), 0) : 0,
  })));
  return { rule, pack };
}

// The mesher's light at every cell of a box: max(sky, block light), from the
// snapshots of the tiles over it (World::snapshot: each section's levels).
function mesherLight(mesher, box) {
  const sections = new Map();
  const tile = (v) => Math.floor((v - 2) / 32);
  for (let tx = tile(box.minX) - 1; tx <= tile(box.maxX) + 1; tx++) {
    for (let tz = tile(box.minZ) - 1; tz <= tile(box.maxZ) + 1; tz++) {
      const bytes = mesher.snapshot(tx, tz);
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      let at = 0;
      const int = () => { const v = view.getInt32(at, true); at += 4; return v; };
      at += 4 * 6;
      const palette = int();
      at += 4 * palette;
      const [min, size, sectionMin, sectionSize] = [0, 1, 2, 3].map(() => [int(), int(), int()]);
      const level = (kind) => {
        const length = [1, 256, 4096][kind];
        const values = bytes.subarray(at, at + length);
        at += length;
        return kind === 0 ? () => values[0] : kind === 1 ? (cell) => values[cell & 255] : (cell) => values[cell];
      };
      for (let sx = sectionMin[0]; sx < sectionMin[0] + sectionSize[0]; sx++) {
        for (let sz = sectionMin[2]; sz < sectionMin[2] + sectionSize[2]; sz++) {
          for (let sy = sectionMin[1]; sy < sectionMin[1] + sectionSize[1]; sy++) {
            const flags = bytes[at++];
            if (flags & 1) at += 8192;
            const sky = level((flags >> 1) & 3);
            const glow = level((flags >> 3) & 3);
            sections.set(`${sx},${sy},${sz}`, { sky, glow, min, size });
          }
        }
      }
    }
  }
  return (x, y, z) => {
    const section = sections.get(`${x >> 4},${y >> 4},${z >> 4}`);
    const inside = section && [x, y, z].every((v, axis) => v >= section.min[axis] && v < section.min[axis] + section.size[axis]);
    if (!inside) return 15;
    const cell = ((y & 15) << 8) | ((z & 15) << 4) | (x & 15);
    return Math.max(section.sky(cell), section.glow(cell));
  };
}

// The viewer's world as the light reads it, and the same blocks as a blueprint.
function fixture(rule) {
  const blocks = new Map(); // 'x,y,z' -> state
  const chunks = new Map();
  const touched = new Set();
  const chunkKey = (x, y, z) => `${x >> 4},${y >> 4},${z >> 4}`;
  const local = (x, y, z) => ((y & 15) * 16 + (z & 15)) * 16 + (x & 15);
  const set = (x, y, z, state) => {
    const key = chunkKey(x, y, z);
    touched.add(key);
    if (!chunks.has(key)) chunks.set(key, new Map());
    if (state === 0) {
      blocks.delete(`${x},${y},${z}`);
      chunks.get(key).delete(local(x, y, z));
    } else {
      blocks.set(`${x},${y},${z}`, state);
      chunks.get(key).set(local(x, y, z), { pos: [x, y, z], light: rule.classOf[state] });
    }
  };
  const blueprint = () => blocksBlueprint([...blocks].map(([key, state]) => [...key.split(',').map(Number), `minecraft:state_${state}`]));
  const take = () => { const keys = [...touched]; touched.clear(); return keys; };
  return { world: { chunks }, blocks, set, blueprint, take };
}

test('see_build\'s light equals the mesher\'s cell for cell on random worlds of rooms, overhangs, shafts and emitters', async () => {
  const { Mesher } = await import('../web/mesher.mjs');
  const { createLight, updateLight } = await import('../web/light.mjs');
  let cells = 0;
  let worlds = 0;
  const seeds = Number(process.env.LIGHT_MESHER_SEEDS || 24);
  for (let seed = 1; seed <= seeds; seed++) {
    const rand = random(seed);
    const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
    const states = kinds(rand);
    const { rule, pack } = await rules(states);
    const mesher = await Mesher.create(WASM, pack);
    const { blocks, set, blueprint, take, world } = fixture(rule);
    const any = () => int(1, states.length - 1);
    const named = (name) => states.findIndex((s) => s.name === name);
    // Minecraft heights: the mesher lights 16 below the lowest block, above
    // the world's floor; every third world stands on the floor.
    const origin = [int(-40, 8), seed % 3 ? int(70, 90) : FLOOR, int(-40, 8)];
    const size = [int(12, 40), int(6, 24), int(12, 40)];
    const inBox = () => origin.map((o, axis) => o + int(0, size[axis] - 1));
    const room = () => {
      const [x0, y0, z0] = inBox();
      const [w, h, d] = [int(3, 9), int(3, 7), int(3, 9)];
      const wall = rand() < 0.5 ? named('stone') : any();
      for (let x = x0; x < x0 + w; x++) for (let y = y0; y < y0 + h; y++) for (let z = z0; z < z0 + d; z++) {
        const shell = x === x0 || x === x0 + w - 1 || y === y0 || y === y0 + h - 1 || z === z0 || z === z0 + d - 1;
        set(x, y, z, shell ? (rand() < 0.06 ? 0 : wall) : 0);
      }
      if (rand() < 0.7) set(x0 + int(1, w - 2), y0 + int(1, h - 2), z0 + int(1, d - 2), named(`light[level=${int(1, 15)}]`));
    };
    const overhang = () => {
      const [x0, y0, z0] = inBox();
      const roof = [named('stone'), named('bottom slab'), named('top slab'), named('leaves'), named('water'), named('stairs'), any()][int(0, 6)];
      for (let x = x0; x < x0 + int(2, 12); x++) for (let z = z0; z < z0 + int(2, 12); z++) set(x, y0, z, roof);
    };
    const shaft = () => {
      const [x, , z] = inBox();
      for (let y = origin[1]; y < origin[1] + size[1]; y++) set(x, y, z, rand() < 0.7 ? 0 : any());
    };
    const scatter = () => { for (let n = int(5, 60); n > 0; n--) set(...inBox(), rand() < 0.15 ? 0 : any()); };
    // Slabs, stairs and other states with face shapes packed together, two
    // layers deep, beside a light.
    const shapedStates = states.flatMap((s, id) => (s.faces ? [id] : []));
    const shaped = () => {
      const [x0, y0, z0] = inBox();
      for (let x = x0; x < x0 + int(2, 8); x++) for (let z = z0; z < z0 + int(2, 8); z++) for (let y = y0; y < y0 + 2; y++) {
        set(x, y, z, rand() < 0.8 ? shapedStates[int(0, shapedStates.length - 1)] : 0);
      }
      set(x0 - 1, y0, z0 - 1, named(`light[level=${int(8, 15)}]`));
    };
    // Blocks at every face and corner of the box.
    for (const x of [origin[0], origin[0] + size[0] - 1]) for (const y of [origin[1], origin[1] + size[1] - 1]) for (const z of [origin[2], origin[2] + size[2] - 1]) set(x, y, z, any());
    for (let n = int(1, 4); n > 0; n--) room();
    for (let n = int(1, 4); n > 0; n--) overhang();
    for (let n = int(0, 3); n > 0; n--) shaft();
    for (let n = int(1, 3); n > 0; n--) shaped();
    scatter();
    const light = createLight(rule, FLOOR);
    for (let round = 0; round < 6; round++) {
      if (round > 0) [room, overhang, shaft, scatter, shaped][int(0, 4)]();
      updateLight(light, world, take());
      if (blocks.size === 0) continue;
      mesher.load(blueprint(), MASK, false);
      const field = light.field;
      const at = mesherLight(mesher, field);
      for (let y = field.minY; y <= field.maxY; y++) for (let z = field.minZ; z <= field.maxZ; z++) for (let x = field.minX; x <= field.maxX; x++) {
        const expected = at(x, y, z);
        const actual = field.lvl[field.at(x, y, z)];
        if (actual !== expected) assert.fail(`seed ${seed} round ${round}: light at ${x},${y},${z} is ${actual}, the mesher's ${expected}`);
        cells += 1;
      }
      worlds += 1;
    }
  }
  assert.equal(worlds, seeds * 6);
  assert.ok(cells > 1e6, `${cells} cells compared`);
});
