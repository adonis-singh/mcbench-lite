'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { World } = require('../server/world');
const { BenchComposer, worldAtRound } = require('../server/bench-world');

function build(blocks) {
  const world = new World();
  for (const [x, y, z, id] of blocks) world.set(x, y, z, id);
  return world.finalize();
}

test('builds sit side by side, each keeping its blocks and their heights', () => {
  const a = build([[100, 5, 40, 'minecraft:stone'], [110, 9, 44, 'minecraft:oak_planks']]);
  const b = build([[-300, 2, -300, 'minecraft:glass']]);
  const { world, layout } = new BenchComposer().compose([{ id: 'a', world: a }, { id: 'b', world: b }]);
  assert.equal(world.size(), 3);
  const [left, right] = layout;
  assert.ok(left.max[0] < right.min[0], 'the builds do not overlap');
  assert.equal(right.min[0] - left.max[0] - 1 >= 16, true, 'a gap separates them');
  assert.deepEqual([left.min[1], left.max[1], right.min[1]], [5, 9, 2]);
  assert.equal(world.get(left.min[0], 5, left.min[2]).id, 'minecraft:stone');
  assert.equal(world.get(left.max[0], 9, left.max[2]).id, 'minecraft:oak_planks');
  assert.equal(world.get(right.min[0], 2, right.min[2]).id, 'minecraft:glass');
});

test('an unchanged build keeps its chunk objects across updates', () => {
  const composer = new BenchComposer();
  const a = build([[0, 0, 0, 'minecraft:stone']]);
  let b = build([[0, 0, 0, 'minecraft:dirt']]);
  const before = composer.compose([{ id: 'a', world: a }, { id: 'b', world: b }]).world;
  b = b.clone();
  b.set(1, 0, 0, 'minecraft:dirt');
  b.finalize();
  const after = composer.compose([{ id: 'a', world: a }, { id: 'b', world: b }]).world;
  const keyOf = (world, id) => [...world.chunks.keys()].find((key) => {
    let found = false;
    world.forEachInChunk(key, (x, y, z, entry) => { found ||= entry.id === id; });
    return found;
  });
  const stone = keyOf(before, 'minecraft:stone');
  assert.ok(stone !== undefined);
  assert.equal(after.chunks.get(stone), before.chunks.get(stone));
  assert.notEqual(after.chunks.get(keyOf(after, 'minecraft:dirt')), before.chunks.get(keyOf(before, 'minecraft:dirt')));
  assert.ok(after.revision > before.revision);
});

test('a round shows each model as it was when that round ended', () => {
  const first = build([[0, 0, 0, 'minecraft:stone']]);
  const member = {
    messages: [{ id: 'u1', role: 'user' }, { id: 'a1', role: 'assistant' }, { id: 'u2', role: 'user' }],
    checkpoints: new Map([['u2', first]]),
    world: build([[0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:stone']]),
  };
  assert.equal(worldAtRound(member, 1), first);
  assert.equal(worldAtRound(member, 2), member.world);
});
