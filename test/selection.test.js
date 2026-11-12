'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { World } = require('../server/world');
const { describeSelection, readSelection } = require('../server/selection');

function house() {
  const world = new World();
  world.fill([0, 0, 0], [9, 5, 9], 'minecraft:stone_bricks');
  world.fill([0, 6, 0], [9, 8, 9], 'minecraft:dark_oak_stairs', { facing: 'north' });
  return world;
}

test('a selection is named by its most common block and where it sits', () => {
  const world = house();
  assert.deepEqual(describeSelection(world, [0, 6, 0], [9, 8, 9]), { blocks: 300, block: 'minecraft:dark_oak_stairs', label: 'Dark oak stairs, top' });
  assert.deepEqual(describeSelection(world, [0, 0, 0], [2, 1, 2]), { blocks: 18, block: 'minecraft:stone_bricks', label: 'Stone bricks, base, north-west side' });
  assert.deepEqual(describeSelection(world, [4, 3, 4], [5, 3, 5]), { blocks: 4, block: 'minecraft:stone_bricks', label: 'Stone bricks, middle' });
  assert.deepEqual(describeSelection(world, [20, 0, 20], [21, 1, 21]), { blocks: 0, block: null, label: 'Empty space' });
});

test('malformed selections are refused', () => {
  assert.throws(() => describeSelection(house(), [0, 0], [1, 1, 1]), /integer corners/);
  assert.throws(() => describeSelection(house(), [3, 0, 0], [1, 1, 1]), /min at most max/);
  assert.throws(() => readSelection({ letter: 'AA', label: 'x', min: [0, 0, 0], max: [0, 0, 0], blocks: 1, block: null }), /letter/);
  assert.deepEqual(readSelection({ letter: 'B', label: 'Glass, top', min: [0, 0, 0], max: [1, 1, 1], blocks: 8, block: 'minecraft:glass' }),
    { letter: 'B', label: 'Glass, top', min: [0, 0, 0], max: [1, 1, 1], blocks: 8, block: 'minecraft:glass' });
  assert.throws(() => readSelection({ letter: 'B', label: 'x', min: [0, 0, 0], max: [0, 0, 0], blocks: 1, block: '../x' }), /main block/);
});

// The box's chunks used to be walked one by one: a box from -2^31 to 2^31
// held the server for good, and corners past the world wrapped onto other
// chunks.
test('a selection costs what the build holds, not the size of its box', () => {
  const world = house();
  const started = performance.now();
  const whole = describeSelection(world, [-4096, -4096, -4096], [4096, 4096, 4096]);
  assert.ok(performance.now() - started < 100, `the whole world took ${performance.now() - started} ms`);
  assert.deepEqual([whole.blocks, whole.block], [900, 'minecraft:stone_bricks']);
  for (const [min, max] of [[[0, 0, 0], [4097, 0, 0]], [[-(2 ** 31), 0, 0], [2 ** 31, 0, 0]]]) {
    assert.throws(() => describeSelection(world, min, max), (error) => error.statusCode === 400 && /within \+\/-4096/.test(error.message));
  }
});

// Equal counts go to the block met first, in the order of the box's chunks
// (x, then y, then z), whatever order the world made its chunks in.
test('a tie is named by the block in the lowest chunk', () => {
  const world = new World();
  world.set(16, 0, 0, 'minecraft:glass');
  world.set(0, 16, 0, 'minecraft:stone');
  world.set(0, 0, 16, 'minecraft:dirt');
  assert.equal(describeSelection(world, [0, 0, 0], [16, 16, 16]).block, 'minecraft:dirt');
  world.set(0, 0, 0, 'minecraft:oak_planks');
  assert.equal(describeSelection(world, [0, 0, 0], [16, 16, 16]).block, 'minecraft:oak_planks');
});
