'use strict';

const assert = require('node:assert');
const { test } = require('node:test');

const { blockSpec, changedKeys, compileChunk } = require('../server/live-placement');
const { validateWorldHeight } = require('../server/minecraft-dimension');
const { World } = require('../server/world');

test('solid chunks compile to one direct fill without a datapack reload', () => {
  const world = new World();
  world.fill([0, 0, 0], [15, 15, 15], 'minecraft:stone', {});
  const plan = compileChunk(world, World.keyOf(0, 0, 0), 64);
  assert.deepStrictEqual(plan.solid, ['fill 0 64 0 15 79 15 minecraft:stone']);
  assert.deepStrictEqual(plan.fragile, []);
});

test('compiled cuboids reproduce mixed blocks and states exactly in a negative chunk', () => {
  const world = new World();
  world.fill([-16, -16, -16], [-1, -1, -1], 'minecraft:stone');
  world.fill([-13, -12, -11], [-4, -3, -2], 'minecraft:air');
  world.fill([-12, -12, -12], [-6, -12, -12], 'minecraft:oak_stairs', { facing: 'west', half: 'top' });
  world.set(-8, -11, -12, 'minecraft:torch');
  world.set(-16, -15, -16, 'minecraft:oak_stairs', { half: 'bottom', facing: 'south' });
  const plan = compileChunk(world, World.keyOf(-1, -1, -1), 64);
  const placed = new Map();
  for (const command of [...plan.solid, ...plan.fragile]) {
    const tokens = command.split(' ');
    const from = tokens.slice(1, 4).map(Number);
    const to = tokens[0] === 'fill' ? tokens.slice(4, 7).map(Number) : from;
    const spec = tokens.at(-1);
    for (let x = from[0]; x <= to[0]; x++) for (let y = from[1]; y <= to[1]; y++) for (let z = from[2]; z <= to[2]; z++) {
      const position = `${x},${y - 64},${z}`;
      assert.equal(placed.has(position), false, `duplicate placement: ${position}`);
      placed.set(position, spec);
    }
  }
  const expected = new Map();
  world.forEach((x, y, z, entry) => expected.set(`${x},${y},${z}`, blockSpec(entry)));
  assert.deepStrictEqual(placed, expected);
  assert.deepStrictEqual(plan.fragile, ['setblock -8 53 -12 minecraft:torch']);
});

test('height validation explains builds that cannot fit in a real world', () => {
  const world = new World();
  world.set(0, 896, 0, 'minecraft:stone', {});
  assert.throws(() => validateWorldHeight(world, 64), /y=960\.\.960/);
});

const ORIGIN = World.keyOf(0, 0, 0);

test('one block edit emits one placement without clearing or rebuilding its neighbors', () => {
  const before = new World();
  for (let x = 0; x < 16; x++) for (let y = 0; y < 16; y++) for (let z = 0; z < 16; z++) {
    before.set(x, y, z, (x + y + z) % 2 ? 'minecraft:stone' : 'minecraft:oak_planks');
  }
  before.finalize();
  const next = before.clone();
  next.set(8, 8, 8, 'minecraft:gold_block');
  next.finalize();
  const plan = compileChunk(next, ORIGIN, 64, before);
  assert.deepStrictEqual(plan, { key: ORIGIN, removed: [], solid: ['setblock 8 72 8 minecraft:gold_block'], fragile: [] });
});

test('incremental cuboids reconstruct additions, removals, and states across independent palettes', () => {
  for (let seed = 1; seed <= 12; seed++) {
    const before = new World();
    const after = new World();
    // Different insertion orders deliberately give equal palette indexes
    // different meanings, as happens when restoring sibling branches.
    before.set(-16, 0, 0, 'minecraft:stone');
    after.set(-16, 0, 0, 'minecraft:oak_planks');
    for (let x = -16; x < 16; x++) for (let z = 0; z < 16; z++) {
      const old = (x * x + z + seed) % 5;
      const next = (x * x + z * 3 + seed) % 7;
      if (old) before.set(x, 0, z, old % 2 ? 'minecraft:stone' : 'minecraft:oak_stairs', old % 2 ? {} : { facing: 'west' });
      if (next) after.set(x, 0, z, next % 2 ? 'minecraft:stone' : 'minecraft:oak_stairs', next % 2 ? {} : { facing: 'east' });
    }
    const placed = new Map();
    before.forEach((x, y, z, entry) => placed.set(`${x},${y},${z}`, blockSpec(entry)));
    for (const key of changedKeys(before, after)) {
      const plan = compileChunk(after, key, 64, before);
      for (const command of [...plan.removed, ...plan.solid, ...plan.fragile]) {
        const tokens = command.split(' ');
        const from = tokens.slice(1, 4).map(Number);
        const to = tokens[0] === 'fill' ? tokens.slice(4, 7).map(Number) : from;
        for (let x = from[0]; x <= to[0]; x++) for (let y = from[1]; y <= to[1]; y++) for (let z = from[2]; z <= to[2]; z++) {
          const position = `${x},${y - 64},${z}`;
          if (tokens.at(-1) === 'minecraft:air') placed.delete(position);
          else placed.set(position, tokens.at(-1));
        }
      }
    }
    const expected = new Map();
    after.forEach((x, y, z, entry) => expected.set(`${x},${y},${z}`, blockSpec(entry)));
    assert.deepStrictEqual(placed, expected);
    const empty = new World();
    for (const key of after.chunks.keys()) {
      const plan = compileChunk(empty, key, 64, after);
      assert.ok(plan.removed.length > 0);
      assert.strictEqual(plan.solid.length + plan.fragile.length, 0);
    }
  }
});

test('fresh sibling branches skip chunks with equal persisted content and palettes', () => {
  const first = new World();
  first.set(0, 0, 0, 'minecraft:stone', {});
  first.finalize();
  first.chunks.get(ORIGIN).persistedHash = 'equal-chunk';

  const sibling = new World();
  sibling.set(0, 0, 0, 'minecraft:stone', {});
  sibling.finalize();
  sibling.chunks.get(ORIGIN).persistedHash = 'equal-chunk';
  assert.deepStrictEqual(changedKeys(first, sibling), []);

  const paletteCollision = new World();
  paletteCollision.set(0, 0, 0, 'minecraft:diamond_block', {});
  paletteCollision.finalize();
  paletteCollision.chunks.get(ORIGIN).persistedHash = 'equal-chunk';
  assert.deepStrictEqual(changedKeys(first, paletteCollision), [ORIGIN]);

  const reorderedPalette = new World();
  reorderedPalette.set(1, 0, 0, 'minecraft:stone', {});
  reorderedPalette.set(0, 0, 0, 'minecraft:oak_planks', {});
  reorderedPalette.set(1, 0, 0, 'minecraft:air', {});
  reorderedPalette.finalize();
  reorderedPalette.chunks.get(ORIGIN).persistedHash = 'left-order';
  const otherOrder = new World();
  otherOrder.set(0, 0, 0, 'minecraft:oak_planks', {});
  otherOrder.finalize();
  otherOrder.chunks.get(ORIGIN).persistedHash = 'right-order';
  assert.deepStrictEqual(changedKeys(reorderedPalette, otherOrder), []);
});

