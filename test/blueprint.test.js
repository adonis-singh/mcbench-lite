'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { blocksBlueprint, blueprintWorld, encodeBlueprint, forEachBlueprintBlock, readBlueprint, worldBlueprint } = require('../server/blueprint');
const { World } = require('../server/world');

test('a voxel world\'s blueprint names its blocks at Minecraft height, properties sorted', () => {
  const world = new World();
  world.set(1, 0, -3, 'minecraft:oak_stairs', { half: 'top', facing: 'east' });
  world.set(1, 1, -3, 'minecraft:stone');
  world.finalize();
  assert.deepEqual(readBlueprint(worldBlueprint(world, 64)), new Map([
    ['1,64,-3', 'minecraft:oak_stairs[facing=east,half=top]'],
    ['1,65,-3', 'minecraft:stone'],
  ]));
  const empty = blocksBlueprint([]);
  assert.equal(empty.readInt32BE(0), 1, 'air alone');
  assert.equal(empty.readInt32BE(empty.length - 4), 0, 'no sections');
});

// Setting every block of a blueprint into a new World, one at a time.
function perBlockWorld(bytes, yOffset) {
  const world = new World();
  forEachBlueprintBlock(bytes, (x, y, z, id, properties) => world.set(x, y - yOffset, z, id, properties));
  return world;
}

// Everything observable about a World: palette order, chunk order, how each chunk stores its blocks.
function shape(world) {
  return {
    palette: world.palette.entries.map((entry) => entry && [entry.id, entry.states]),
    blockCount: world.blockCount,
    chunks: [...world.chunks].map(([key, chunk]) => [key, chunk.count, Boolean(chunk.dense), chunk.dense ? [...chunk.dense] : [...chunk.sparse]]),
  };
}

function randomBlueprint(seed) {
  let state = seed;
  const random = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 2 ** 32; };
  const names = ['minecraft:stone', 'minecraft:oak_stairs[facing=north,half=bottom]', 'minecraft:water[level=3]', 'minecraft:torch', 'minecraft:oak_fence[east=true,west=false]'];
  const palette = ['minecraft:air', ...names];
  const sections = [];
  for (const [sx, sy, sz] of [[0, 0, 0], [0, 1, 0], [-1, 0, 2], [3, -2, -4], [0, 5, 1]]) {
    const density = random() < 0.5 ? 0.9 : 0.02;
    const indices = new Uint16Array(4096);
    for (let i = 0; i < 4096; i += 1) if (random() < density) indices[i] = 1 + Math.floor(random() * names.length);
    sections.push([[sx, sy, sz], indices]);
  }
  return encodeBlueprint(palette, sections, [0, 0, 0, 0, 0, 0]);
}

test('the blueprint of the engine\'s world becomes the World setting its blocks one by one would make', () => {
  for (const seed of [1, 2, 3, 4]) {
    const bytes = randomBlueprint(seed);
    assert.deepEqual(shape(blueprintWorld(bytes, 64)), shape(perBlockWorld(bytes, 64)));
    const shifted = blueprintWorld(bytes, 5);
    const expected = perBlockWorld(bytes, 5);
    const blocks = (world) => { const out = []; world.forEach((x, y, z, entry) => out.push(`${x},${y},${z} ${entry.id}${JSON.stringify(entry.states)}`)); return out.sort(); };
    assert.deepEqual(blocks(shifted), blocks(expected));
    assert.equal(shifted.size(), expected.size());
  }
  const shared = perBlockWorld(randomBlueprint(1), 64).palette;
  assert.equal(blueprintWorld(randomBlueprint(1), 64, shared).palette, shared);
  assert.equal(blueprintWorld(randomBlueprint(1), 64, shared).palette.entries.length, shared.entries.length);
  assert.equal(blueprintWorld(encodeBlueprint(['minecraft:air'], [], [0, 0, 0, 0, 0, 0]), 64).size(), 0);
});

test('a blueprint block beyond the world limits is refused as setting it would be', () => {
  const indices = new Uint16Array(4096).fill(1);
  const bytes = encodeBlueprint(['minecraft:air', 'minecraft:stone'], [[[257, 0, 0], indices]], [0, 0, 0, 0, 0, 0]);
  assert.throws(() => blueprintWorld(bytes, 0), /out of range/);
  assert.throws(() => perBlockWorld(bytes, 0), /out of range/);
});
