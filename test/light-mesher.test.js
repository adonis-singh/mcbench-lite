'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');

const { MODULE } = require('../scripts/build-renderer');
const { blocksBlueprint } = require('../server/blueprint');
const { minecraftLive } = require('../server/minecraft-live');
const { lightPack } = require('./helpers/mesher-pack');
const { opaqueStates } = require('./helpers/block-table');

// see_build's light (web/light.mjs) against the live viewer's mesher
// (renderer/, the committed WebAssembly module the live viewer runs), cell
// for cell: one light in both views. The mesher's light is checked against
// Paper's own light engine by scripts/mesher-parity.js --paper, and against
// its own light lit from nothing in its tests (renderer/src/world.rs). Both
// light a build standing alone in open sky, above the world's floor (y = -64
// here, the mesher's); the mesher keeps sky and block light apart, the page
// the brighter of the two. Both take each state's facts from a block table
// (McbenchBlockTable.java's: its emission, light block and six face shapes)
// and its pairs of face shapes that close a face (Shapes.faceShapeOccludes).
// The worlds are lit under tables of three kinds: states and pairs drawn at
// random, among them partial shapes that close a face together (a bottom
// slab's half side beside a top slab's, as Minecraft's do); and, where the
// data folder holds Paper and the tools folder the JDK, Minecraft's own
// states and face shapes, read from Paper's classes.

const WASM = fs.readFileSync(MODULE);
const MASK = { minX: -100000, maxX: 100000, minZ: -100000, maxZ: 100000 };
const FLOOR = -64;

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

// A table drawn at random: face shapes 0 empty, 1 full, 2 and 3 a bottom
// and a top slab's half sides (which close a face together, either way
// round) and more partial shapes; a full face closes against every face and
// two empty ones never close; every other ordered pair closes or not at
// random. Block states lit as Minecraft's are (emission, light block, and
// for a state that is conditionally full opaque its six face shapes, down,
// up, north, south, west, east), and as many more made at random.
function randomTable(rand) {
  const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
  const shapes = 4 + int(0, 5);
  const occludes = Uint8Array.from({ length: shapes * shapes }, (_, at) => {
    const [a, b] = [Math.floor(at / shapes), at % shapes];
    if (a === 1 || b === 1) return 1;
    if (a === 0 && b === 0) return 0;
    if ((a === 2 && b === 3) || (a === 3 && b === 2)) return 1;
    return rand() < 0.3 ? 1 : 0;
  });
  const partial = () => int(2, shapes - 1);
  const states = [
    { name: 'air', emission: 0, lightBlock: 0 },
    { name: 'stone', emission: 0, lightBlock: 15, faces: [1, 1, 1, 1, 1, 1] },
    { name: 'glass', emission: 0, lightBlock: 0 },
    { name: 'water', emission: 0, lightBlock: 1 },
    { name: 'leaves', emission: 0, lightBlock: 1 },
    { name: 'wall', emission: 0, lightBlock: 0 },
    { name: 'glowstone', emission: 15, lightBlock: 15, faces: [1, 1, 1, 1, 1, 1] },
    { name: 'torch', emission: 14, lightBlock: 0 },
    { name: 'bottom slab', emission: 0, lightBlock: 0, faces: [1, 0, 2, 2, 2, 2] },
    { name: 'top slab', emission: 0, lightBlock: 0, faces: [0, 1, 3, 3, 3, 3] },
    { name: 'waterlogged slab', emission: 0, lightBlock: 1, faces: [1, 0, 2, 2, 2, 2] },
    { name: 'stairs', emission: 0, lightBlock: 0, faces: [1, partial(), 1, partial(), partial(), partial()] },
    { name: 'upside-down stairs', emission: 0, lightBlock: 0, faces: [partial(), 1, partial(), partial(), partial(), 1] },
    { name: 'double slab', emission: 0, lightBlock: 15, faces: [1, 1, 1, 1, 1, 1] },
  ];
  for (let level = 1; level <= 15; level++) states.push({ name: `light[level=${level}]`, emission: level, lightBlock: 0 });
  for (let n = 0; n < 12; n++) {
    states.push({
      name: `random ${n}`, emission: rand() < 0.4 ? int(1, 15) : 0, lightBlock: [0, 0, 1, 1, 2, int(3, 14), 15][int(0, 6)],
      faces: rand() < 0.6 ? Array.from({ length: 6 }, () => int(0, shapes - 1)) : undefined,
    });
  }
  return { states, shapes, occludes };
}

// Minecraft's own table (server/block-states.js), and states of it: the
// kinds randomTable names, every state of oak slabs and stairs, and states
// drawn at random, half of them among those with face shapes.
function minecraftTable(blockStates, rand) {
  const of = (name, id, properties = {}) => {
    const state = blockStates.idOf(id, properties);
    const faces = Array.from(blockStates.faces.subarray(state * 6, state * 6 + 6));
    return { name, emission: blockStates.emission[state], lightBlock: blockStates.lightBlock[state], faces: faces.some(Boolean) ? faces : undefined };
  };
  const states = [
    of('air', 'minecraft:air'), of('stone', 'minecraft:stone'), of('glass', 'minecraft:glass'), of('water', 'minecraft:water'),
    of('leaves', 'minecraft:oak_leaves'), of('wall', 'minecraft:cobblestone_wall'), of('glowstone', 'minecraft:glowstone'), of('torch', 'minecraft:torch'),
    of('bottom slab', 'minecraft:oak_slab', { type: 'bottom' }), of('top slab', 'minecraft:oak_slab', { type: 'top' }),
    of('waterlogged slab', 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'true' }),
    of('stairs', 'minecraft:oak_stairs', { half: 'bottom' }), of('upside-down stairs', 'minecraft:oak_stairs', { half: 'top' }),
    of('double slab', 'minecraft:oak_slab', { type: 'double' }),
  ];
  for (let level = 1; level <= 15; level++) states.push(of(`light[level=${level}]`, 'minecraft:light', { level: String(level) }));
  for (const half of ['bottom', 'top']) for (const facing of ['north', 'south', 'west', 'east']) {
    for (const shape of ['straight', 'inner_left', 'inner_right', 'outer_left', 'outer_right']) states.push(of(`oak stairs ${half} ${facing} ${shape}`, 'minecraft:oak_stairs', { half, facing, shape }));
  }
  const shaped = [];
  for (let state = 0; state < blockStates.count; state++) if (blockStates.faces.subarray(state * 6, state * 6 + 6).some(Boolean)) shaped.push(state);
  const names = [...blockStates.ids.keys()];
  for (let n = 0; n < 40; n++) {
    const state = n % 2 ? shaped[Math.floor(rand() * shaped.length)] : Math.floor(rand() * blockStates.count);
    const faces = Array.from(blockStates.faces.subarray(state * 6, state * 6 + 6));
    states.push({ name: names[state], emission: blockStates.emission[state], lightBlock: blockStates.lightBlock[state], faces: faces.some(Boolean) ? faces : undefined });
  }
  return { states, shapes: blockStates.shapes, occludes: blockStates.occludes };
}

// The page's rule for a table and the mesher's pack of its states.
async function rules({ states, shapes, occludes }) {
  const { lightRule } = await import('../web/light.mjs');
  const faces = new Uint8Array(states.length * 6);
  states.forEach((state, id) => state.faces && faces.set(state.faces, id * 6));
  const rule = lightRule({
    emission: Uint8Array.from(states, (s) => s.emission),
    lightBlock: Uint8Array.from(states, (s) => s.lightBlock),
    faces,
    shapes,
    occludes,
  });
  const pack = lightPack(states.map((s) => ({ emission: s.emission, lightBlock: s.lightBlock, faces: s.faces })), shapes, occludes);
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

// Lights random worlds of rooms, overhangs, shafts, emitters and shaped
// blocks packed together with both, under the table `tableOf(rand)` makes
// for each seed, and compares every cell after each of six rounds of edits.
async function sameLight(seeds, tableOf) {
  const { Mesher } = await import('../web/mesher.mjs');
  const { createLight, updateLight } = await import('../web/light.mjs');
  let cells = 0;
  let worlds = 0;
  for (let seed = 1; seed <= seeds; seed++) {
    const rand = random(seed);
    const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
    const table = tableOf(rand);
    const { states } = table;
    const { rule, pack } = await rules(table);
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
  return cells;
}

const SEEDS = Number(process.env.LIGHT_MESHER_SEEDS || 24);

test('see_build\'s light equals the mesher\'s cell for cell under random tables, partial face shapes that close a face among them', async () => {
  const cells = await sameLight(SEEDS, randomTable);
  assert.ok(cells > 1e6, `${cells} cells compared`);
});

// Minecraft's block table as the data folder holds it (block-table-minecraft.test.js
// and the app's first start make it).
const TABLE = minecraftLive.blockTableFile();

test('see_build\'s light equals the mesher\'s cell for cell under Minecraft\'s states and face shapes', { skip: !fs.existsSync(TABLE) && 'needs the block table made from Paper\'s classes in the data folder' }, async () => {
  const blockStates = opaqueStates(fs.readFileSync(TABLE, 'utf8'));
  const cells = await sameLight(SEEDS / 2, (rand) => minecraftTable(blockStates, rand));
  assert.ok(cells > 5e5, `${cells} cells compared`);
});
