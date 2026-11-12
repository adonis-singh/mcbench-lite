'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { entityIndex } = require('./helpers/entity-index');

const near = (actual, expected, message) => {
  assert.equal(actual.length, expected.length, message);
  actual.forEach((value, i) => assert.ok(Math.abs(value - expected[i]) < 1e-9, `${message}: ${actual} is not ${expected}`));
};

const entity = (id, extra = {}) => ({ id, type: 'minecraft:item', pos: [0, 0, 0], yaw: 0, pitch: 0, width: 0.25, height: 0.25, ...extra });

// ---- bookkeeping and smoothing ----

test('the tracker adds entities, keeps them by id and removes the ones a frame leaves out', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker();
  assert.deepEqual(tracker.set([entity(1), entity(2)], 0), { added: [1, 2], removed: [] });
  assert.deepEqual(tracker.set([entity(2), entity(3)], 50), { added: [3], removed: [1] });
  assert.deepEqual(tracker.sample(50).map((state) => state.id).sort(), [2, 3]);
  assert.deepEqual(tracker.set([], 100), { added: [], removed: [2, 3] });
  assert.deepEqual(tracker.sample(100), []);
});

test('an entity glides to its new position over the time the last frame took and stays there', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker();
  tracker.set([entity(1, { pos: [0, 10, 0] })], 1000);
  tracker.set([entity(1, { pos: [2, 10, 0] })], 1050);
  near(tracker.sample(1050)[0].pos, [0, 10, 0], 'at the frame');
  near(tracker.sample(1075)[0].pos, [1, 10, 0], 'halfway');
  near(tracker.sample(1100)[0].pos, [2, 10, 0], 'a frame later');
  near(tracker.sample(1500)[0].pos, [2, 10, 0], 'later still, without running on');
  assert.equal(tracker.moving(1075), true);
  assert.equal(tracker.moving(1100), false);
});

test('a frame that arrives mid-glide continues from where the entity is shown', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker();
  tracker.set([entity(1, { pos: [0, 0, 0] })], 0);
  tracker.set([entity(1, { pos: [2, 0, 0] })], 50);
  tracker.set([entity(1, { pos: [3, 0, 0] })], 75);
  near(tracker.sample(75)[0].pos, [1, 0, 0], 'where the first glide had reached');
  near(tracker.sample(87.5)[0].pos, [2, 0, 0], 'half of the way on, over the 25 ms the frame took');
  near(tracker.sample(100)[0].pos, [3, 0, 0]);
});

test('an entity that moves further than a few blocks in one frame jumps', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker();
  tracker.set([entity(1, { pos: [0, 0, 0] }), entity(2, { pos: [0, 0, 0] })], 0);
  tracker.set([entity(1, { pos: [3.9, 0, 0] }), entity(2, { pos: [4.1, 0, 0] })], 50);
  near(tracker.sample(50).find((state) => state.id === 1).pos, [0, 0, 0], 'a short move glides');
  near(tracker.sample(50).find((state) => state.id === 2).pos, [4.1, 0, 0], 'a teleport does not');
});

test('angles glide the short way round and the glide time follows the frame spacing within bounds', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker({ maxInterval: 250 });
  tracker.set([entity(1, { yaw: 350, pitch: 0 })], 0);
  tracker.set([entity(1, { yaw: 10, pitch: 40 })], 1000);
  const half = tracker.sample(1125)[0];
  assert.ok(Math.abs(((half.yaw + 180) % 360) - 180) < 1e-9, `half way from 350 to 10 is 0, not ${half.yaw}`);
  assert.equal(half.pitch, 20);
  near([tracker.sample(1250)[0].yaw], [370], 'a spacing of a second is cut to 250 ms');
});

test('a frame with a repeated id or an entity without a position is refused', async () => {
  const { EntityTracker } = await import('../web/entities.mjs');
  const tracker = new EntityTracker();
  assert.throws(() => tracker.set([entity(1), entity(1)], 0), /twice/);
  assert.throws(() => tracker.set([entity(1, { pos: [0, 0] })], 0), /position/);
  assert.throws(() => tracker.set([entity(1, { yaw: undefined })], 0), /angles/);
  assert.deepEqual(tracker.sample(0), [], 'a refused frame changes nothing');
});

// ---- the index ----

test('the index must name only textures, models and blocks it holds', async () => {
  const { validateIndex } = await import('../web/entity-models.mjs');
  validateIndex(entityIndex());
  const broken = (change) => {
    const index = entityIndex();
    change(index);
    return index;
  };
  assert.throws(() => validateIndex(broken((i) => { i.models['block/stone'].elements[0].faces.up.texture = 'block/nope'; })), /block\/nope/);
  assert.throws(() => validateIndex(broken((i) => { i.textures['block/stone'] = [250, 0, 16, 16]; })), /outside the atlas/);
  assert.throws(() => validateIndex(broken((i) => { i.blocks['minecraft:stone'].variants[''].model = 'block/nope'; })), /block\/nope/);
  assert.throws(() => validateIndex(broken((i) => { i.items['minecraft:redstone'].sprite = 'item/nope'; })), /item\/nope/);
  assert.throws(() => validateIndex(broken((i) => { i.entities['minecraft:minecart'] = 'entity/nope'; })), /entity\/nope/);
});

test('a block state picks the variant that agrees with its properties, or the multipart parts that apply', async () => {
  const { blockPlacements, parseState, matchesWhen } = await import('../web/entity-models.mjs');
  const index = entityIndex();
  assert.deepEqual(parseState('minecraft:slab[facing=east,half=top]'), { id: 'minecraft:slab', props: { facing: 'east', half: 'top' } });
  assert.deepEqual(blockPlacements(index, 'minecraft:slab[facing=east]'), [{ model: 'block/slab', y: 90 }]);
  assert.deepEqual(blockPlacements(index, 'minecraft:slab'), [{ model: 'block/slab' }], 'a state with no properties takes the first variant');
  assert.throws(() => blockPlacements(index, 'minecraft:slab[facing=west]'), /No model of minecraft:slab fits/);
  assert.throws(() => blockPlacements(index, 'minecraft:nope'), /no block minecraft:nope/);
  assert.deepEqual(blockPlacements(index, 'minecraft:fence').map((p) => p.model), ['block/post']);
  assert.deepEqual(blockPlacements(index, 'minecraft:fence[north=true,east=false,south=false]').map((p) => p.y || 0), [0, 0]);
  assert.deepEqual(blockPlacements(index, 'minecraft:fence[north=false,east=false,south=true]').map((p) => p.y || 0), [0, 90]);
  assert.equal(matchesWhen({ shape: 'straight|inner' }, { shape: 'inner' }), true);
  assert.equal(matchesWhen({ shape: '!straight' }, { shape: 'straight' }), false);
  assert.equal(matchesWhen({ AND: [{ a: 'true' }, { b: 'true' }] }, { a: 'true', b: 'false' }), false);
  assert.deepEqual(blockPlacements(index, 'minecraft:air'), []);
});

test('a block model becomes quads on the atlas, tinted where a face asks and turned by the blockstate', async () => {
  const { EntityIndex, blockMesh, addModel, MeshBuilder } = await import('../web/entity-models.mjs');
  const entityIndexed = new EntityIndex(entityIndex());
  const stone = blockMesh(entityIndexed, 'minecraft:stone');
  assert.equal(stone.position.length / 3, 36, 'six faces of two triangles');
  for (let i = 0; i < stone.uv.length; i += 2) {
    assert.ok(stone.uv[i] >= 0 && stone.uv[i] <= 16 / 256 && stone.uv[i + 1] >= 0 && stone.uv[i + 1] <= 16 / 128, 'a stone uv stays on the stone texture');
  }
  assert.ok(stone.position.every((value) => Math.abs(value) <= 0.5), 'the block is centered on the origin');
  assert.equal(blockMesh(entityIndexed, 'minecraft:air'), null);

  const grass = blockMesh(entityIndexed, 'minecraft:grass_block');
  const colors = new Set();
  for (let i = 0; i < grass.color.length; i += 3) colors.add(Array.from(grass.color.slice(i, i + 3), (v) => v.toFixed(3)).join());
  assert.deepEqual([...colors].sort(), ['0.569,0.741,0.349', '1.000,1.000,1.000'], 'only the top is tinted');

  const turned = new MeshBuilder();
  addModel(turned, entityIndexed, 'block/slab', { y: 90 });
  near(Array.from(turned.normal.slice(0, 3)), [1, 0, 0], 'a north face turned by y=90 faces east');
  const tilted = new MeshBuilder();
  addModel(tilted, entityIndexed, 'block/tilted');
  near(Array.from(tilted.normal.slice(0, 3)), [-Math.SQRT1_2, 0, -Math.SQRT1_2], 'an element rotation turns the normal too');

  const plain = new MeshBuilder();
  addModel(plain, entityIndexed, 'block/slab');
  // The slab lists north then west; its west face is rotated, so its first corner takes the uv another corner has.
  const north = plain.uv.slice(0, 2);
  const west = plain.uv.slice(12, 14);
  near(Array.from(north), [0, 0], 'the north face starts at the texture corner');
  near(Array.from(west), [0, 16 / 128], 'a face rotated 90 degrees starts at the next corner');
});

test('a sprite is a thin slab of its texture', async () => {
  const { EntityIndex, spriteMesh } = await import('../web/entity-models.mjs');
  const mesh = spriteMesh(new EntityIndex(entityIndex()), 'item/redstone');
  assert.equal(mesh.position.length / 3, 36, 'two faces and four edges');
  const zs = Array.from({ length: mesh.position.length / 3 }, (_, i) => mesh.position[i * 3 + 2]);
  assert.equal(Math.max(...zs), 1 / 32);
  assert.equal(Math.min(...zs), -1 / 32);
  for (let i = 0; i < mesh.uv.length; i += 2) {
    assert.ok(mesh.uv[i] >= 80 / 256 && mesh.uv[i] <= 96 / 256 && mesh.uv[i + 1] >= 0 && mesh.uv[i + 1] <= 16 / 128, 'a uv stays on the sprite');
  }
});

// ---- what each kind of entity is drawn as ----

const translationOf = (matrix) => matrix.slice(12, 15);

test('an item entity is its sprite or small block, in a stack as Minecraft draws, spinning and bobbing', async () => {
  const { EntityIndex, EntityModels, copiesOf } = await import('../web/entity-models.mjs');
  const models = new EntityModels(new EntityIndex(entityIndex()));
  assert.deepEqual([0, 1, 2, 16, 17, 32, 33, 48, 49, 64].map(copiesOf), [1, 1, 2, 2, 3, 3, 4, 4, 5, 5]);
  const sprite = models.describe(entity(7, { item: 'minecraft:redstone', count: 20 }));
  assert.equal(sprite.parts.length, 3);
  assert.equal(sprite.animated, true);
  const heights = [0, 0.5, 1, 1.5, 2].map((seconds) => translationOf(sprite.parts[0].update(seconds))[1]);
  assert.ok(Math.max(...heights) - Math.min(...heights) > 0.05, 'it bobs');
  const spin = (seconds) => sprite.parts[0].update(seconds).slice(0, 3);
  assert.notDeepEqual(spin(0), spin(1), 'it turns');
  const block = models.describe(entity(8, { item: 'minecraft:stone', count: 1 }));
  assert.equal(block.parts.length, 1);
  assert.ok(Math.abs(block.parts[0].update(0).slice(0, 3).reduce((s, v) => s + v * v, 0) - 0.0625) < 1e-9, 'a block is a quarter of a block across');
  const unknown = models.describe(entity(9, { item: 'minecraft:bundle_of_nothing', width: 0.25, height: 0.25 }));
  assert.deepEqual(unknown.parts[0].box, { width: 0.25, height: 0.25, label: 'bundle_of_nothing' });
});

test('falling blocks, primed TNT, minecarts and boats are drawn from their models', async () => {
  const { EntityIndex, EntityModels } = await import('../web/entity-models.mjs');
  const models = new EntityModels(new EntityIndex(entityIndex()));
  const falling = models.describe(entity(1, { type: 'minecraft:falling_block', block: 'minecraft:grass_block' }));
  assert.equal(falling.parts[0].mesh.position.length / 3, 36);
  near(translationOf(falling.parts[0].matrix), [0, 0.5, 0], 'it stands on the entity position');

  const tnt = models.describe(entity(2, { type: 'minecraft:tnt', fuse: 80 }));
  assert.equal(tnt.parts.length, 2);
  assert.notEqual(tnt.parts[1].update(0, { fuse: 80 }), null, 'white for five ticks');
  assert.equal(tnt.parts[1].update(0, { fuse: 77 }), null, 'then not for five');
  const scale = (fuse) => tnt.parts[0].update(0, { fuse })[0];
  assert.equal(scale(80), 1);
  assert.ok(scale(0) > 1.19, 'it swells as it runs out');

  const plain = models.describe(entity(3, { type: 'minecraft:minecart' }));
  assert.equal(plain.parts.length, 1);
  const loaded = models.describe(entity(4, { type: 'minecraft:minecart', block: 'minecraft:tnt' }));
  assert.equal(loaded.parts.length, 2, 'the display block rides inside');
  near(translationOf(loaded.parts[1].matrix), [0, 0.656, 0]);
  const cart = plain.parts[0].mesh;
  assert.equal(cart.position.length / 3, 5 * 36);
  const ys = Array.from({ length: cart.position.length / 3 }, (_, i) => cart.position[i * 3 + 1]);
  assert.ok(Math.min(...ys) > 0 && Math.max(...ys) < 0.8, 'the cart stands on the ground, under three quarters of a block');
  near(translationOf(plain.orient(90, 0)), [0, 0, 0]);

  const boat = models.describe(entity(5, { type: 'minecraft:oak_boat' }));
  assert.equal(boat.parts[0].mesh.position.length / 3, 5 * 36);
});

test('armor stands, arrows, frames and paintings are placed by their own angles and facing', async () => {
  const { EntityIndex, EntityModels, transformPoint } = await import('../web/entity-models.mjs');
  const models = new EntityModels(new EntityIndex(entityIndex()));
  const stand = models.describe(entity(1, { type: 'minecraft:armor_stand', height: 1.975 }));
  const ys = Array.from({ length: stand.parts[0].mesh.position.length / 3 }, (_, i) => stand.parts[0].mesh.position[i * 3 + 1]);
  assert.ok(Math.abs(Math.min(...ys)) < 0.01, 'a stand has its feet on the position');
  assert.ok(Math.abs(Math.max(...ys) - 1.876) < 0.01, `and its head 1.876 up, not ${Math.max(...ys)}`);
  const small = models.describe(entity(2, { type: 'minecraft:armor_stand', height: 0.9875 }));
  assert.ok(Math.max(...Array.from(small.parts[0].mesh.position).filter((_, i) => i % 3 === 1)) < 1.05, 'a small stand is half as tall');
  near(transformPoint(stand.orient(0, 0), [0, 0, -1]), [0, 0, 1], 'yaw 0 turns the model to face south');
  near(transformPoint(stand.orient(90, 0), [0, 0, -1]), [-1, 0, 0], 'yaw 90 faces west');

  const arrow = models.describe(entity(3, { type: 'minecraft:arrow', height: 0.5 }));
  near(transformPoint(arrow.orient(0, 0), [1, 0, 0]), [0, 0, 1], 'an arrow with yaw 0 flies south');
  assert.ok(transformPoint(arrow.orient(0, 30), [1, 0, 0])[1] < 0, 'and down for a positive pitch');

  const frame = models.describe(entity(4, { type: 'minecraft:item_frame', facing: 'east', item: 'minecraft:redstone', rotation: 2 }));
  near(translationOf(frame.parts[0].matrix), [0.46875, 0, 0], 'the frame stands out from the wall');
  near(translationOf(frame.parts[1].matrix), [0.04, 0, 0], 'the item sits just in front of the entity');
  assert.throws(() => models.describe(entity(5, { type: 'minecraft:item_frame', facing: 'sideways' })), /faces sideways/);

  const painting = models.describe(entity(6, { type: 'minecraft:painting', facing: 'north', width: 2, height: 1, variant: 'minecraft:alban' }));
  const xs = Array.from({ length: painting.parts[0].mesh.position.length / 3 }, (_, i) => painting.parts[0].mesh.position[i * 3]);
  assert.equal(Math.max(...xs) - Math.min(...xs), 2);
  const bare = models.describe(entity(7, { type: 'minecraft:painting', facing: 'north', width: 1, height: 1 }));
  assert.ok(bare.parts[0].mesh, 'a painting the list gives no variant for shows its back');
});

test('every other entity type is its bounding box with its name', async () => {
  const { EntityIndex, EntityModels } = await import('../web/entity-models.mjs');
  const models = new EntityModels(new EntityIndex(entityIndex()));
  const zombie = models.describe(entity(1, { type: 'minecraft:zombie', width: 0.6, height: 1.95 }));
  assert.deepEqual(zombie.parts[0].box, { width: 0.6, height: 1.95, label: 'zombie' });
  near(translationOf(zombie.parts[0].matrix), [0, 0.975, 0], 'the box stands on the position');
  assert.equal(zombie.animated, false);
});

test('an entity is drawn again when what it looks like changes, not when it moves', async () => {
  const { appearance } = await import('../web/entity-models.mjs');
  const base = entity(1, { item: 'minecraft:redstone', count: 1 });
  assert.equal(appearance(base), appearance({ ...base, pos: [5, 5, 5], yaw: 40, fuse: 3 }));
  assert.notEqual(appearance(base), appearance({ ...base, item: 'minecraft:stone' }));
  assert.notEqual(appearance(base), appearance({ ...base, count: 20 }));
});
