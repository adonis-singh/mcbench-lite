'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { entityCapture, square, IDENTITY, translation, levels } = require('./helpers/entity-capture');

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

// ---- what an entity is drawn as (entity-draw.mjs) ----

// A capture of the shape McbenchEntities.java writes: a kind "cart" whose
// variant has a block draw and a model of two parts, both turned by yaw
// about the origin and the block moved by the display offset; a kind "stand"
// whose head part turns with Head.x and whose held item follows the arm; an
// orb kind of two icons; a TNT kind folded from fuse 2 in a period of 2; a
// painting whose quad is lit by its own cell; and an item with copies.
const TEXTURES = ['minecraft:entity/cart', 'minecraft:entity/stand', 'minecraft:item/rod', 'minecraft:painting/art', 'minecraft:block/stone'];
const turn = (value) => ({ value, axis: [0, 1, 0], point: [0, 0, 0], rate: -1 });
const plain = { block: levels(), sky: levels(), probe: [0, 0.5, 0] };
function capture() {
  return entityCapture({
    textures: TEXTURES,
    meshes: [[square('minecraft:entity/cart')], [square('minecraft:entity/stand')], [square('minecraft:item/rod')], [square('minecraft:painting/art')]],
    poses: [IDENTITY, translation(0, 1, 0), translation(0.5, 0, 0), translation(2, 0, 0)],
    kinds: {
      'minecraft:minecart': {
        light: plain, by: [],
        variants: { '': { base: { yaw: 0, pitch: 0, offset: 6 }, draws: [
          { kind: 'block', light: 'entity', overlay: 655360, pose: 1, motions: [turn('yaw'), { value: 'offset', vector: [0, 0.0625, 0] }], block: 'entity' },
          { kind: 'parts', layer: 'entity_cutout_no_cull', light: 'entity', overlay: 655360, pose: 0, motions: [turn('yaw')], parts: [
            { path: '', pose: [0, 0, 0, 0, 0, 0, 1, 1, 1], deps: {}, mesh: 0 },
            { path: '/box', parent: '', pose: [16, 0, 0, 0, 0, 0, 1, 1, 1], deps: {}, mesh: 0 },
          ] },
        ] } },
      },
      'minecraft:armor_stand': {
        light: plain, by: ['small'],
        variants: { false: { base: { yaw: 0, 'Head.x': 0 }, draws: [
          { kind: 'parts', layer: 'entity_cutout_no_cull', light: 'entity', overlay: 655360, pose: 0, motions: [turn('yaw')], parts: [
            { path: '', pose: [0, 0, 0, 0, 0, 0, 1, 1, 1], deps: {}, mesh: 1 },
            { path: '/head', parent: '', pose: [0, 16, 0, 0, 0, 0, 1, 1, 1], deps: { 'Head.x': [0, 0, 0, Math.PI / 180, 0, 0, 0, 0, 0] }, mesh: 1 },
          ] },
          { kind: 'item', layer: 'item_entity_translucent_cull', light: 'entity', overlay: 655360, attach: { draw: 0, part: '/head', matrix: 2 }, slot: 'mainhand', context: 'third_person_right_hand' },
        ] } },
        equipment: {},
      },
      'minecraft:experience_orb': {
        light: { block: levels().map((l) => Math.min(l + 7, 15)), sky: levels(), probe: [0, 0, 0] }, by: ['icon'], icons: [[0, 0], [3, 1]],
        variants: { 0: { base: {}, draws: [{ kind: 'mesh', layer: 'item_entity_translucent_cull', light: 'entity', pose: 0, motions: [], mesh: 0, billboard: [0, 0.1, 0] }] }, 1: { base: {}, draws: [{ kind: 'mesh', layer: 'item_entity_translucent_cull', light: 'entity', pose: 1, motions: [], mesh: 0, billboard: [0, 0.1, 0] }] } },
      },
      'minecraft:tnt': {
        light: plain, by: ['fuse'], fold: { value: 'fuse', from: 2, period: 2 },
        variants: Object.fromEntries([0, 1, 2, 3].map((fuse) => [fuse, { base: {}, draws: [{ kind: 'block', light: 'entity', overlay: fuse === 2 ? 655375 : 655360, pose: fuse, motions: [], block: 'entity' }] }])),
      },
      'minecraft:painting': {
        light: plain, by: ['variant', 'facing'],
        variants: { 'minecraft:art,north': { base: {}, draws: [{ kind: 'mesh', layer: 'entity_solid', light: { cells: [[0, 0, -1]] }, pose: 0, motions: [], mesh: 3 }] } },
      },
      'minecraft:item': { light: plain, by: [], variants: {}, tiers: [1, 2, 2, 3] },
    },
    items: {
      'minecraft:rod': {
        third_person_right_hand: [{ layer: 'item_entity_translucent_cull', overlay: 655360, pose: 0, mesh: 2 }],
        dropped: [{ layer: 'item_entity_translucent_cull', overlay: 655360, pose: 0, mesh: 2 }],
        copies: [3, 2],
      },
    },
  });
}

const stoneBlock = () => [{ ...square('minecraft:block/stone'), color: [1, 1, 1, 1], layer: 'block' }];
const view = (light = () => [15, 15]) => ({ light, blockQuads: stoneBlock });
const corners = (quad) => [0, 1, 2, 3].map((k) => quad.corners.slice(k * 3, k * 3 + 3));
const at = (m, [x, y, z]) => [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]];
const nearAll = (actual, expected, message) => actual.forEach((corner, i) => near(corner, expected[i], `${message}, corner ${i}`));

test('a draw is placed by its pose, moved by the values its motions name, and a model\'s parts each by their own pose under their parent\'s', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const cart = { id: 1, type: 'minecraft:minecart', pos: [0, 0, 0], yaw: 0, pitch: 0, ticks: 0, block: 'minecraft:stone', offset: 10 };
  const [block, root, box] = draw.geometry(data, cart, view());
  // The block: its pose (one up), then 4 more of the display offset's
  // sixteenths up; the parts: the root's square, and /box sixteen pixels
  // (one block) east.
  nearAll(corners(block), [[0, 1.25, 0], [1, 1.25, 0], [1, 2.25, 0], [0, 2.25, 0]], 'the block');
  nearAll(corners(root), [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], 'the root part');
  nearAll(corners(box), [[1, 0, 0], [2, 0, 0], [2, 1, 0], [1, 1, 0]], 'the box part');
  assert.equal(block.texture, 'minecraft:block/stone', 'a block draw is the entity\'s block');
  assert.equal(root.texture, 'minecraft:entity/cart');
});

test('the motions every draw shares are the entity\'s frame, applied as it moves; the rest are its geometry', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const cart = { id: 1, type: 'minecraft:minecart', pos: [3, 4, 5], yaw: 90, pitch: 0, ticks: 0, block: 'minecraft:stone', offset: 6 };
  const variant = data.kind('minecraft:minecart').variants[''];
  assert.deepEqual(draw.frameMotions(variant).map((motion) => motion.value), ['yaw'], 'the yaw turns both draws alike, first; the offset only the block');
  const frame = draw.frame(data, cart, { pos: cart.pos, yaw: 90, pitch: 0, age: 0 }, null);
  // A turn of -90 degrees about y (rate -1), then at the position: x goes to z.
  const [, root] = draw.geometry(data, cart, view());
  nearAll(corners(root).map((corner) => at(frame, corner)), [[3, 4, 5], [3, 4, 6], [3, 5, 6], [3, 5, 5]], 'the root part, turned and placed');
  assert.equal(draw.appearance(data, cart, () => [15, 15]), draw.appearance(data, { ...cart, yaw: 10, pos: [0, 0, 0] }, () => [15, 15]), 'a turn or a move is not another look');
  assert.notEqual(draw.appearance(data, cart, () => [15, 15]), draw.appearance(data, { ...cart, offset: 7 }, () => [15, 15]), 'another offset is');
  assert.notEqual(draw.appearance(data, cart, () => [15, 15]), draw.appearance(data, cart, () => [3, 15]), 'and so is another light');
});

test('a part that changes with a value turns by its change, and a draw that follows the part goes with it', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const stand = { id: 2, type: 'minecraft:armor_stand', pos: [0, 0, 0], yaw: 0, pitch: 0, ticks: 0, small: false, pose: [[90, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]], equipment: { mainhand: 'minecraft:rod' } };
  const [, head, item] = draw.geometry(data, stand, view());
  // The head one block up, turned 90 degrees about x (ZYX with x alone): y
  // goes to z.
  nearAll(corners(head), [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1]], 'the head, turned by Head.x');
  // The item: the head's pose, then half a block along the head's own x.
  nearAll(corners(item), [[0.5, 1, 0], [1.5, 1, 0], [1.5, 1, 1], [0.5, 1, 1]], 'the held item, on the head');
  assert.equal(item.texture, 'minecraft:item/rod');
  const empty = draw.geometry(data, { ...stand, equipment: {} }, view());
  assert.equal(empty.length, 2, 'an empty hand draws no item');
});

test('a variant is picked by its fields, a folded value by its period, an orb by the icon its value takes', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const tnt = (fuse) => draw.variantOf(data.kind('minecraft:tnt'), { type: 'minecraft:tnt', fuse });
  assert.equal(tnt(1).draws[0].pose, 1);
  assert.equal(tnt(2).draws[0].pose, 2);
  assert.equal(tnt(5).draws[0].pose, 3, 'fuse 5 draws as fuse 3');
  assert.equal(tnt(8).draws[0].pose, 2, 'fuse 8 draws as fuse 2');
  const orb = (value) => draw.variantOf(data.kind('minecraft:experience_orb'), { type: 'minecraft:experience_orb', value });
  assert.equal(orb(2).draws[0].pose, 0);
  assert.equal(orb(3).draws[0].pose, 1);
  assert.throws(() => draw.variantOf(data.kind('minecraft:painting'), { type: 'minecraft:painting', variant: 'minecraft:art', facing: 'up' }), /No variant/);
  assert.throws(() => draw.variantOf(data.kind('minecraft:tnt'), { type: 'minecraft:tnt' }), /does not say its fuse/);
});

test('an entity is lit through its kind\'s light from its probe\'s cell, a painting\'s quad from its own, and a flash blends white in', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const asked = [];
  const light = (x, y, z) => {
    asked.push([x, y, z]);
    return [x === 4 && y === 0 && z === 4 ? 9 : 3, 12];
  };
  const [orb] = draw.geometry(data, { id: 3, type: 'minecraft:experience_orb', pos: [2.5, 0.2, 2.5], yaw: 0, pitch: 0, ticks: 0, value: 1 }, view(light));
  assert.deepEqual(asked.at(-1), [2, 0, 2], 'the orb reads its probe cell');
  assert.deepEqual(orb.light, [10, 12], 'its block light seven brighter, as its kind says');
  asked.length = 0;
  const [art] = draw.geometry(data, { id: 4, type: 'minecraft:painting', pos: [4.5, 0.5, 5.03], yaw: 0, pitch: 0, ticks: 0, variant: 'minecraft:art', facing: 'north' }, view(light));
  assert.deepEqual(art.light, [9, 12], 'the painting\'s quad in its cell, one north of the entity\'s');
  assert.deepEqual(draw.lightCells(data, { type: 'minecraft:painting', pos: [4.5, 0.5, 5.03], variant: 'minecraft:art', facing: 'north' }), [[4, 1, 5], [4, 0, 4]]);
  const [lit] = draw.geometry(data, { id: 5, type: 'minecraft:tnt', pos: [0, 0, 0], yaw: 0, pitch: 0, ticks: 0, fuse: 2, block: 'minecraft:stone' }, view());
  assert.deepEqual(lit.overlay, [1, 1, 1, 63 / 255], 'OverlayTexture: u 15 keeps a quarter');
  assert.deepEqual(draw.overlayColor(655360), [1, 1, 1, 1], 'u 0 keeps all');
});

test('a dropped stack draws its copies by count, and spins and bobs with its age', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const data = new draw.EntityData(capture());
  const drop = (count) => draw.geometry(data, { id: 6, type: 'minecraft:item', pos: [0, 0, 0], yaw: 0, pitch: 0, ticks: 0, item: 'minecraft:rod', count }, view());
  assert.equal(drop(1).length, 1);
  assert.equal(drop(2).length, 2);
  assert.equal(drop(4).length, 3);
  assert.equal(drop(64).length, 3, 'past the tiers, the last');
  nearAll(corners(drop(4)[2]), [[0.5, 0, 0], [1.5, 0, 0], [1.5, 1, 0], [0.5, 1, 0]], 'the third copy moved as the second copy pose says');
  const entity = { id: 6, type: 'minecraft:item', pos: [0, 0, 0], item: 'minecraft:rod', count: 1 };
  const offset = draw.bobOffset(6);
  const frame = draw.frame(data, entity, { pos: [0, 0, 0], age: 20 }, null);
  const spin = draw.itemSpin(20, offset);
  near([frame[0], frame[2], frame[13]], [Math.cos(spin), -Math.sin(spin), draw.itemBob(20, offset) - draw.itemBob(0, 0)], 'turned by the spin about y, lifted by the bob');
});

test('Minecraft\'s small functions: Mth.sin\'s table, a minecart\'s offset by id, an orb\'s colour by age', async () => {
  const draw = await import('../web/entity-draw.mjs');
  // Mth.sin(x) = SIN[(int) (x * 10430.378F) & 65535].
  assert.equal(draw.mthSin(0), 0);
  assert.equal(draw.mthSin(Math.PI / 2), Math.fround(Math.sin((Math.trunc(Math.fround(Math.fround(Math.PI / 2) * Math.fround(10430.378))) * Math.PI * 2) / 65536)));
  // Id 0's seed is 0: each axis (0.5 / 8 - 0.5) * 0.004.
  near(draw.minecartOffset(0), [-0.00175, -0.00175, -0.00175].map(Math.fround), 'id 0');
  const [r, g, b] = draw.orbColor(0);
  assert.deepEqual([r, g], [127, 255]);
  assert.ok(b > 0 && b < 51);
});

// ---- the live viewer's layer (entities.mjs) ----

// Just enough of BlueMap's three.js for the layer: objects with matrices,
// geometry that keeps its attributes.
function fakeBlueMap() {
  class Object3D {
    constructor(geometry, material) { this.children = []; this.matrix = { elements: new Array(16).fill(0) }; this.geometry = geometry; this.material = material; }
    add(...objects) { this.children.push(...objects); }
    remove(object) { this.children = this.children.filter((child) => child !== object); }
    traverse(visit) { visit(this); for (const child of this.children) child.traverse(visit); }
    clone() { const copy = new Object3D(this.geometry, this.material); copy.children = this.children.map((child) => child.clone()); return copy; }
  }
  class BufferGeometry {
    constructor() { this.attributes = {}; }
    setAttribute(name, attribute) { this.attributes[name] = attribute; }
    computeBoundingSphere() {}
    dispose() { this.disposed = true; }
  }
  class BufferAttribute { constructor(array, size) { this.array = array; this.itemSize = size; } }
  class Thing { constructor(...args) { this.args = args; } }
  const Three = { Group: Object3D, Mesh: Object3D, BufferGeometry, BufferAttribute, Texture: Thing, ShaderMaterial: Thing, NearestFilter: 0, ClampToEdgeWrapping: 0, DoubleSide: 0 };
  return {
    Three,
    HIRES_VERTEX_SHADER: 'attribute float ao; void main() { vNormal = normal; vColor = color; }',
    HIRES_FRAGMENT_SHADER: 'varying vec3 vColor; void main() { color.rgb *= vColor.rgb; }',
  };
}

async function entityLayer() {
  const { EntityLayer } = await import('../web/entities.mjs');
  const { EntityData } = await import('../web/entity-draw.mjs');
  const viewer = { markers: { add() {}, remove() {} }, data: { uniforms: {} }, camera: { matrixWorld: { elements: [...IDENTITY] } }, redraws: 0, redraw() { this.redraws += 1; } };
  // The mesher: every cell in light 15 of block and sky, every block stone's square.
  const asked = [];
  const mesher = {
    async entities(cells, states) {
      asked.push({ cells, states });
      const models = { count: 2, position: new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0]), uv: new Float32Array(12), color: new Float32Array(6).fill(1), material: new Int32Array([0, 0]) };
      return { light: cells.map(() => [15, 15]), blocks: Object.fromEntries(states.map((state) => [state, models])) };
    },
  };
  const data = new EntityData(capture());
  const layer = new EntityLayer({ BlueMap: fakeBlueMap(), app: { mapViewer: viewer, events: new EventTarget() }, assets: { data, textures: ['minecraft:block/stone'], atlas: {} }, mesher });
  return { layer, viewer, asked, data };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const cart = (pos, extra = {}) => ({ id: 1, type: 'minecraft:minecart', pos, yaw: 0, pitch: 0, ticks: 0, block: 'minecraft:stone', offset: 6, width: 0.98, height: 0.7, ...extra });

test('the live viewer\'s layer draws an entity\'s geometry in its frame: the shared module\'s quads, moved by its frame each frame', async () => {
  const draw = await import('../web/entity-draw.mjs');
  const { layer, asked, data } = await entityLayer();
  layer.set([cart([2, 3, 4], { yaw: 90 })], 0);
  await settle();
  assert.deepEqual(asked[0].states, ['minecraft:stone'], 'the block it shows is asked of the mesher');
  assert.deepEqual(asked[0].cells, [[2, 3, 4]], 'and the light at its probe');
  const object = layer.objects.get(1).object;
  const positions = object.children[0].geometry.attributes.position.array;
  const shared = draw.geometry(data, cart([2, 3, 4], { yaw: 90 }), { light: () => [15, 15], blockQuads: (state) => layer.blocks.get(state) });
  // Each quad two triangles: corners 0, 1, 2 and 0, 2, 3.
  shared.forEach((quad, q) => [0, 1, 2, 0, 2, 3].forEach((k, i) => near(Array.from(positions.slice((q * 6 + i) * 3, (q * 6 + i) * 3 + 3)), quad.corners.slice(k * 3, k * 3 + 3), `quad ${q} vertex ${i}`)));
  near(object.matrix.elements, draw.frame(data, cart([2, 3, 4], { yaw: 90 }), { pos: [2, 3, 4], yaw: 90, pitch: 0, age: 0 }, null), 'its frame');
});

test('a frame of the same look keeps the entity\'s geometry and moves it; another look builds it again', async () => {
  const { layer, viewer } = await entityLayer();
  layer.set([cart([0, 0, 0])], 0);
  await settle();
  const first = layer.objects.get(1).object;
  const redraws = viewer.redraws;
  layer.set([cart([0, 0, 0])], 50);
  await settle();
  assert.equal(viewer.redraws, redraws, 'a still world draws nothing more');
  layer.set([cart([1, 0, 0], { yaw: 30 })], 100);
  await settle();
  assert.equal(layer.objects.get(1).object, first, 'moved and turned, the same geometry');
  layer.set([cart([1, 0, 0], { offset: 9 })], 150);
  await settle();
  assert.notEqual(layer.objects.get(1).object, first, 'another display offset, built again');
  layer.set([], 200);
  assert.equal(layer.objects.size, 0, 'an entity gone is removed');
});
