'use strict';

const { test } = require('node:test');
const assert = require('node:assert');

const { MAX_BLOCKS, World } = require('../server/world');

test('clone copies only changed chunks and leaves the base immutable', () => {
  const base = new World();
  base.fill([0, 0, 0], [31, 15, 15], 'minecraft:stone', {});
  base.finalize();

  const candidate = base.clone();
  candidate.set(1, 1, 1, 'minecraft:oak_planks', {});
  candidate.set(40, 1, 1, 'minecraft:stone', {});

  assert.strictEqual(base.get(1, 1, 1).id, 'minecraft:stone');
  assert.strictEqual(base.get(40, 1, 1), null);
  assert.deepStrictEqual(candidate.diff(base), { added: 1, removed: 0, changed: 1 });
  assert.ok(candidate.chunks.get(World.keyOf(0, 0, 0)) !== base.chunks.get(World.keyOf(0, 0, 0)));
  assert.ok(candidate.chunks.get(World.keyOf(1, 0, 0)) === base.chunks.get(World.keyOf(1, 0, 0)));
});

test('chunk manifest round-trips and shares content-addressed payloads', () => {
  const world = new World();
  world.fill([-20, 0, -20], [20, 3, 20], 'minecraft:stone', {});
  const files = new Map();
  const manifest = world.serializeManifest((blocks) => {
    const key = JSON.stringify(blocks);
    files.set(key, blocks);
    return key;
  });
  const restored = World.fromManifest(manifest, (key) => files.get(key));

  assert.strictEqual(restored.size(), world.size());
  assert.deepStrictEqual(restored.bounds(), world.bounds());
  assert.strictEqual(restored.get(-20, 0, -20).id, 'minecraft:stone');
  assert.strictEqual(restored.get(21, 0, 0), null);
});

test('three million blocks fit exactly and the next block fails fast', () => {
  const world = new World();
  world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  assert.strictEqual(world.size(), MAX_BLOCKS);
  assert.throws(
    () => world.set(300, 0, 0, 'minecraft:stone', {}),
    /3000000 block limit/,
  );
});

test('a positive-coordinate 1920x1080 canvas fits inside world bounds', () => {
  const base = new World().finalize();
  const world = base.clone();
  world.fill([0, 0, 0], [1919, 1079, 0], 'minecraft:black_concrete', {});
  assert.strictEqual(world.size(), 1920 * 1080);
  assert.deepStrictEqual(world.bounds(), { min: [0, 0, 0], max: [1919, 1079, 0] });
  assert.deepStrictEqual(world.renderLod(), { axis: 2, scale: 4 });
  let renderBlocks = 0;
  for (const key of world.chunks.keys()) {
    renderBlocks += world.chunkSnapshot(key, true, true, world.renderLod()).blocks.length;
  }
  assert.strictEqual(renderBlocks, (1920 / 4) * (1080 / 4));
});

test('a world with writes since its last finalize cannot be cloned, since it would write its clone\'s chunks', () => {
  const base = new World();
  base.set(0, 0, 0, 'minecraft:stone');
  assert.throws(() => base.clone(), /cannot be cloned: finalize it first/);
  base.finalize();
  const clone = base.clone();
  base.set(1, 0, 0, 'minecraft:stone');
  assert.equal(clone.get(1, 0, 0), null);
});

test('a world keeps the ticks it was settled with through its manifest and its clones', () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.finalize();
  world.settleTicks = 40;
  const chunks = new Map();
  const manifest = world.serializeManifest((pairs) => { const hash = String(chunks.size); chunks.set(hash, pairs); return hash; });
  const loaded = World.fromManifest(JSON.parse(JSON.stringify(manifest)), (hash) => chunks.get(hash));
  assert.equal(loaded.settleTicks, 40);
  assert.equal(loaded.clone().settleTicks, 40);
  assert.equal(loaded.reference().settleTicks, 40);
});

// What a refused write must leave as it was.
function contents(world) {
  return {
    size: world.size(),
    bounds: world.bounds(),
    chunks: [...world.chunks].map(([key, chunk]) => [key, chunk.count, chunk.version]),
    changed: [...world.changedChunks],
    owned: [...world.ownedChunks],
    palette: world.palette.entries.length,
  };
}

test('a fill the block limit refuses leaves the world as it was, in a chunk that did not exist too', () => {
  const world = new World();
  world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  world.finalize();
  const full = contents(world);
  // The box starts in a chunk the world does not have.
  assert.throws(() => world.fill([320, 0, 0], [321, 0, 0], 'minecraft:oak_planks', {}), /3000000 block limit/);
  assert.deepStrictEqual(contents(world), full);
  assert.strictEqual(world.chunks.has(World.keyOf(20, 0, 0)), false);
  assert.throws(() => world.set(320, 0, 0, 'minecraft:oak_planks', {}), /3000000 block limit/);
  assert.deepStrictEqual(contents(world), full);
  // Blocks that are there already are not added: the same box fills again.
  world.fill([0, 0, 0], [299, 199, 49], 'minecraft:oak_planks', {});
  assert.strictEqual(world.size(), MAX_BLOCKS);

  // A box that reaches the limit part-way through writes none of its blocks.
  const partial = new World();
  partial.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  partial.fill([0, 0, 0], [9, 0, 0], 'minecraft:air', {});
  partial.finalize();
  const before = contents(partial);
  assert.throws(() => partial.fill([0, 0, 0], [399, 0, 0], 'minecraft:oak_planks', {}), /3000000 block limit/);
  assert.deepStrictEqual(contents(partial), before);
  assert.strictEqual(partial.get(300, 0, 0), null);
  // Ten blocks of room take ten blocks.
  partial.fill([0, 0, 0], [20, 0, 0], 'minecraft:oak_planks', {});
  assert.strictEqual(partial.size(), MAX_BLOCKS);
  assert.strictEqual(partial.get(5, 0, 0).id, 'minecraft:oak_planks');
});

test('the block limit error says how many blocks the world has and the call would add', () => {
  const world = new World();
  world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  world.fill([0, 0, 0], [3, 0, 0], 'minecraft:air', {});
  assert.throws(
    () => world.fill([296, 0, 0], [305, 0, 0], 'minecraft:oak_planks', {}),
    /^Error: Build exceeds the 3000000 block limit: the world has 2999996 blocks and this call would add 6$/,
  );
  world.fill([0, 0, 0], [3, 0, 0], 'minecraft:stone', {});
  assert.throws(
    () => world.set(0, 0, 50, 'minecraft:stone', {}),
    /^Error: Build exceeds the 3000000 block limit: the world has 3000000 blocks and this call would add 1$/,
  );
});
