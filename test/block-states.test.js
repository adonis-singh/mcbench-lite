'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { World } = require('../server/world');
const { testBlockStates } = require('./helpers/block-table');

// A table of states written for these tests: a cube with six full faces that
// blocks all light, one that also gives light, a clear block, a block that
// gives light and blocks none, and a slab whose half sides close a face only
// beside the other half (face shapes 2 and 3).
const CUBE = { full: 63, lightBlock: 15 };
const states = testBlockStates([
  { name: 'test:cube', ...CUBE },
  { name: 'test:lamp[lit=false]', ...CUBE },
  { name: 'test:lamp[lit=true]', ...CUBE, emission: 15 },
  { name: 'test:clear' },
  { name: 'test:glow[level=9,wet=false]', emission: 9 },
  { name: 'test:glow[level=9,wet=true]', emission: 9, lightBlock: 1 },
  { name: 'test:glow[level=3,wet=false]', emission: 3 },
  { name: 'test:glow[level=3,wet=true]', emission: 3, lightBlock: 1 },
  { name: 'test:slab[type=bottom]', full: 1, faces: [1, 0, 2, 2, 2, 2] },
  { name: 'test:slab[type=top]', full: 2, faces: [0, 1, 3, 3, 3, 3] },
], { shapes: 4, pairs: [[2, 3], [3, 2]] });
const fact = (id, properties = {}) => {
  const state = states.idOf(id, properties);
  return { emission: states.emission[state], lightBlock: states.lightBlock[state], full: states.fullFaces[state] };
};

test('a state is found by its block and properties, its block\'s default filling the properties it leaves out', () => {
  assert.equal(states.idOf('minecraft:air', {}), 0);
  assert.deepEqual(fact('test:glow'), { emission: 9, lightBlock: 0, full: 0 });
  assert.deepEqual(fact('test:glow', { wet: 'true' }), { emission: 9, lightBlock: 1, full: 0 });
  assert.deepEqual(fact('test:glow', { wet: 'true', level: '3' }), { emission: 3, lightBlock: 1, full: 0 });
  assert.deepEqual([fact('test:lamp'), fact('test:lamp', { lit: 'true' })], [{ emission: 0, lightBlock: 15, full: 63 }, { emission: 15, lightBlock: 15, full: 63 }]);
  assert.throws(() => states.idOf('test:glow', { level: '16' }), /no block state test:glow\[level=16,wet=false\]/);
  assert.throws(() => states.idOf('test:nothing', {}), /no block test:nothing/);
});

test('the bytes the page reads hold every state\'s facts and the face shape pairs', () => {
  const bytes = states.bytes();
  const [count, shapes] = new Uint32Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + 8));
  assert.deepEqual([count, shapes], [states.count, 4]);
  assert.equal(bytes.length, 8 + count * 9 + shapes * shapes);
  const bottom = states.idOf('test:slab', { type: 'bottom' });
  const top = states.idOf('test:slab', { type: 'top' });
  const glow = states.idOf('test:glow', {});
  assert.deepEqual([bytes[8 + glow], bytes[8 + count + glow], bytes[8 + 2 * count + bottom]], [9, 0, 1]);
  const faces = bytes.subarray(8 + count * 3);
  const occludes = bytes.subarray(8 + count * 9);
  // A bottom half beside a top half closes the face between them; two bottom halves do not.
  const east = (state) => faces[state * 6 + 5];
  const west = (state) => faces[state * 6 + 4];
  assert.equal(occludes[east(bottom) * shapes + west(top)], 1);
  assert.equal(occludes[east(bottom) * shapes + west(bottom)], 0);
  assert.equal(occludes[faces[bottom * 6] * shapes], 1, 'its full bottom closes the face below it alone');
});

// The blocks of a world's chunk as see_build's page is sent them.
const shown = (world, key) => {
  const chunk = world.chunkSnapshot(key, states);
  return chunk.blocks.map(([x, y, z, index]) => `${x},${y},${z} ${chunk.palette[index].id}`).sort();
};

test('a block is left out only where it and the six blocks around it have six full faces', () => {
  const world = new World();
  world.fill([0, 0, 0], [2, 2, 2], 'test:cube');
  const key = World.keyOf(0, 0, 0);
  assert.equal(shown(world, key).length, 26);
  assert.ok(!shown(world, key).includes('1,1,1 test:cube'));
  // A block that gives light, a clear one or a slab beside it shows it.
  for (const [id, properties] of [['test:glow', {}], ['test:clear', {}], ['test:slab', { type: 'bottom' }]]) {
    world.set(1, 2, 1, id, properties);
    assert.ok(shown(world, key).includes('1,1,1 test:cube'), id);
  }
  // A lit cube hidden on every side gives light no one sees: left out.
  world.set(1, 2, 1, 'test:cube');
  world.set(1, 1, 1, 'test:lamp', { lit: 'true' });
  assert.ok(!shown(world, key).some((block) => block.startsWith('1,1,1 ')));
  // Each palette entry carries its state id.
  const chunk = world.chunkSnapshot(key, states);
  assert.deepEqual(chunk.palette.map(({ id, state }) => [id, state]), [['test:cube', states.idOf('test:cube', {})]]);
});

test('a chunk\'s version changes where a block across its faces hides or shows its edge, and only there', () => {
  // A wall of cubes across the border between two chunks at x = 15 | 16.
  const world = new World();
  world.fill([14, 0, 0], [17, 2, 2], 'test:cube');
  const left = World.keyOf(0, 0, 0);
  const right = World.keyOf(1, 0, 0);
  const before = world.shownVersion(left, states);
  assert.ok(!shown(world, left).includes('15,1,1 test:cube'));
  // Inside the other chunk, past its edge layer: the same version, the same blocks.
  world.set(17, 1, 1, 'test:clear');
  assert.equal(world.shownVersion(left, states), before);
  // On its edge layer: the block across shows.
  world.set(16, 1, 1, 'test:clear');
  const after = world.shownVersion(left, states);
  assert.notEqual(after, before);
  assert.ok(shown(world, left).includes('15,1,1 test:cube'));
  assert.equal(world.chunkSnapshot(left, states).version, after);
  // Put back: the version it had.
  world.set(16, 1, 1, 'test:cube');
  assert.equal(world.shownVersion(left, states), before);
  // The other chunk gone: nothing hides the edge.
  for (let x = 16; x <= 17; x++) for (let y = 0; y <= 2; y++) for (let z = 0; z <= 2; z++) world.set(x, y, z, 'minecraft:air');
  assert.ok(!world.chunks.has(right) || world.chunks.get(right).count === 0);
  assert.notEqual(world.shownVersion(left, states), before);
  assert.ok(shown(world, left).includes('15,1,1 test:cube'));
});
