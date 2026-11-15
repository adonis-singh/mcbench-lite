'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');
const { MODULE } = require('../scripts/build-renderer');
const { testBlockStates } = require('./helpers/block-table');
const { testBlockTints } = require('./helpers/block-tints');
const { mesherPack } = require('./helpers/mesher-pack');
const { viewerHarness } = require('./helpers/viewer');
const { entityCapture, square, IDENTITY, translation, levels } = require('./helpers/entity-capture');

// see_build's viewer (web/viewer.js) draws the engine's entities with the
// live viewer's own geometry (web/entity-draw.mjs over the bake's capture)
// after the blocks, from the bake's entity atlas, each quad lit by
// see_build's light at the cell its kind reads; the block an entity shows is
// the live viewer's mesher's faces of it alone (Mesher.lone). A world with
// no entity loads none of it.
const ALL = 63;
const STATES = testBlockStates([{ name: 'minecraft:stone', full: ALL, lightBlock: 15 }, { name: 'minecraft:lamp[lit=false]', renderer: 'minecraft:lamp' }, { name: 'minecraft:shrine', renderer: 'minecraft:shrine' }]);
const TINTS = testBlockTints(STATES);
const TEXTURES = ['down', 'up', 'north', 'south', 'west', 'east', 'quad'].map((side) => `minecraft:block/lamp_${side}`);
const cube = (texture) => ({
  textures: { all: texture },
  elements: [{ from: [0, 0, 0], to: [16, 16, 16], faces: Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((face) => [face, { texture: '#all', cullface: face }])) }],
});
const ASSETS = {
  'blocks.json': { stone: [{}, {}], lamp: [{ lit: ['false', 'true'] }, { lit: 'false' }], shrine: [{}, {}] },
  'block_definitions.json': { stone: { variants: { '': { model: 'minecraft:block/stone' } } } },
  'models.json': { 'block/stone': cube('minecraft:block/stone') },
  'atlas_map.json': { 'block/stone': [0, 0, 16, 16] },
};
// A cart: a model of one square part, turned by yaw (rate -1 about y), and
// the lamp it shows half a block up.
const CAPTURE = entityCapture({
  textures: ['minecraft:entity/cart', ...TEXTURES],
  meshes: [[square('minecraft:entity/cart')]],
  poses: [IDENTITY, translation(0, 0.5, 0)],
  atlas: { width: 32, height: 16, rects: Object.fromEntries(['minecraft:entity/cart', ...TEXTURES].map((name, i) => [name, [i % 2 * 16, 0, 16, 16]])) },
  kinds: {
    'minecraft:minecart': {
      light: { block: levels(), sky: levels(), probe: [0, 0.5, 0] }, by: [],
      variants: { '': { base: { yaw: 0 }, draws: [
        { kind: 'parts', layer: 'entity_cutout_no_cull', light: 'entity', overlay: 655360, pose: 0, motions: [{ value: 'yaw', axis: [0, 1, 0], point: [0, 0, 0], rate: -1 }], parts: [{ path: '', pose: [0, 0, 0, 0, 0, 0, 1, 1, 1], deps: {}, mesh: 0 }] },
        { kind: 'block', light: 'entity', overlay: 655360, pose: 1, motions: [{ value: 'yaw', axis: [0, 1, 0], point: [0, 0, 0], rate: -1 }], block: 'entity' },
      ] } },
    },
  },
});

let open;
const fetched = [];
test.before(async () => {
  open = await viewerHarness(STATES, ASSETS, TINTS, { wasm: fs.readFileSync(MODULE), pack: mesherPack(), textures: TEXTURES, entities: CAPTURE }, fetched);
});

// A world of one stone and `entities`, made ready and drawn: the uploads
// of making it (the solid entities' quads are uploaded once, as they are
// built, after the chunks) and of drawing a frame.
async function draw(entities) {
  const { viewer, uploads } = await open();
  viewer.applyDelta({ chunks: [{ key: 'a', version: 1, origin: [0, 64, 0], palette: [{ id: 'minecraft:stone', states: {}, state: STATES.ids.get('minecraft:stone') }], blocks: [[0, 0, 0, 0]] }], removed: [] });
  viewer.setEntities(entities);
  await viewer.prepare();
  uploads.length = 0;
  viewer.flush();
  const made = uploads.splice(0);
  viewer.renderNow();
  return { made, drawn: uploads };
}

const close = (a, b) => a.length === b.length && a.every((value, i) => Math.abs(value - b[i]) < 1e-5);

test('a world without entities loads none of the entities\' capture and draws none', async () => {
  fetched.length = 0;
  const { drawn } = await draw([]);
  assert.ok(!fetched.some((url) => /mcbench-entities/.test(url)), `fetched ${fetched}`);
  assert.equal(drawn.length, 0, 'a frame of meshes already uploaded uploads nothing');
});

test('an entity is the live viewer\'s quads of it, placed by its frame, from the entities\' atlas, lit by its cell', async () => {
  const entity = { id: 1, type: 'minecraft:minecart', pos: [3.5, 66, 0.5], yaw: 90, pitch: 0, ticks: 0, block: 'minecraft:lamp[lit=false]', offset: 6, width: 1, height: 1 };
  const { made, drawn } = await draw([entity]);
  // The entity program's six attributes, last of what was made.
  const [position, texture, , color] = made.slice(-6).map((upload) => upload.data);
  assert.ok(drawn.every((upload) => upload.target !== made.at(-1).target), 'a frame of solid entities uploads no vertices');
  const page = await import('../web/entity-draw.mjs');
  const { Mesher } = await import('../web/mesher.mjs');
  const mesher = await Mesher.create(fs.readFileSync(MODULE), mesherPack());
  const data = new page.EntityData(CAPTURE);
  const quads = page.geometry(data, entity, { light: () => [15, 15], blockQuads: (state) => page.loneQuads(mesher, TEXTURES, state) });
  const frame = page.frame(data, entity, { pos: entity.pos, yaw: 90, pitch: 0, age: 0 }, null);
  assert.equal(position.length, quads.length * 12, 'every quad: the cart\'s, and the lamp\'s faces');
  quads.forEach((quad, q) => [0, 1, 2, 3].forEach((k) => {
    const expected = page.transformPoint(frame, quad.corners[k * 3], quad.corners[k * 3 + 1], quad.corners[k * 3 + 2]);
    assert.ok(close(position.slice((q * 4 + k) * 3, (q * 4 + k) * 3 + 3), expected), `quad ${q} corner ${k} at ${position.slice((q * 4 + k) * 3, (q * 4 + k) * 3 + 3)}, the module's ${expected}`);
  }));
  // The cart's square: its texture's rect (0, 0, 16, 16) over the 32 by 16 atlas.
  assert.ok(close(texture.slice(0, 8), [0, 0, 0.5, 0, 0.5, 1, 0, 1]), `textured ${texture.slice(0, 8)}`);
  // Open sky: full light, see_build's curve at 15.
  assert.ok(close(color.slice(0, 3), [1, 1, 1]), `coloured ${color.slice(0, 3)}`);
});
