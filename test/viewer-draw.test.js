'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { testBlockStates } = require('./helpers/block-table');
const { viewerHarness } = require('./helpers/viewer');

// What see_build's viewer (web/viewer.js) draws of a block, and in which
// pass, by the block table's facts written for the test: glass that skips
// rendering beside glass and is drawn blended; water (a fluid alone, drawn
// blended, skipping beside every state that holds water); a stair that holds
// water and one that does not; stone. Every model is a cube whose faces
// cull toward their own sides, so a block's quads are its faces drawn.
const ALL = 63;
const WATER = 'minecraft:water[level=0]';
const WET = 'minecraft:oak_stairs[waterlogged=true]';
const STATES = testBlockStates([
  { name: 'minecraft:stone', full: ALL, lightBlock: 15 },
  { name: 'minecraft:glass', blended: true },
  { name: WATER, model: false, fluid: WATER, fluidBlended: true, lightBlock: 1 },
  { name: 'minecraft:oak_stairs[waterlogged=false]' },
  { name: WET, fluid: WATER, fluidBlended: true, lightBlock: 1 },
], {
  skips: [['minecraft:glass', 'minecraft:glass', ALL], [WATER, WATER, ALL], [WATER, WET, ALL]],
});
const cube = (texture) => ({
  textures: { all: texture },
  elements: [{
    from: [0, 0, 0],
    to: [16, 16, 16],
    faces: Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((face) => [face, { texture: '#all', cullface: face }])),
  }],
});
const ASSETS = {
  'blocks.json': { stone: [{}, {}], glass: [{}, {}], water: [{ level: ['0'] }, { level: '0' }], oak_stairs: [{ waterlogged: ['false', 'true'] }, { waterlogged: 'false' }] },
  'block_definitions.json': {
    stone: { variants: { '': { model: 'minecraft:block/stone' } } },
    glass: { variants: { '': { model: 'minecraft:block/glass' } } },
    oak_stairs: { variants: { '': { model: 'minecraft:block/oak_stairs' } } },
  },
  'models.json': { 'block/stone': cube('minecraft:block/stone'), 'block/glass': cube('minecraft:block/glass'), 'block/oak_stairs': cube('minecraft:block/oak_planks') },
  'atlas_map.json': {
    'block/stone': [0, 0, 16, 16], 'block/glass': [16, 0, 16, 16], 'block/oak_planks': [0, 0, 16, 16],
    'block/water_still': [16, 0, 16, 16], 'block/water_flow': [16, 0, 16, 16],
  },
};

let open;
test.before(async () => {
  open = await viewerHarness(STATES, ASSETS);
});

// The quads drawn of one world, by pass: [opaque, blended].
async function draw(blocks) {
  const { viewer, chunks } = await open();
  const names = [...new Set(blocks.map(([, , , name]) => name))];
  const palette = names.map((name) => {
    const [id, properties] = name.split('[');
    return { id, states: properties ? Object.fromEntries(properties.slice(0, -1).split(',').map((pair) => pair.split('='))) : {}, state: STATES.ids.get(name) };
  });
  viewer.applyDelta({ chunks: [{ key: 'a', version: 1, origin: [0, 64, 0], palette, blocks: blocks.map(([x, y, z, name]) => [x, y, z, names.indexOf(name)]) }], removed: [] });
  viewer.flush();
  const count = (mesh) => chunks().reduce((sum, chunk) => sum + chunk[mesh].quadVertices() / 4, 0);
  return [count('mesh'), count('transparentMesh')];
}

test('glass hides its faces toward glass, by skipRendering, and is drawn blended', async () => {
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:glass']]), [0, 6]);
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:glass'], [1, 0, 0, 'minecraft:glass']]), [0, 10]);
  // Beside stone, its face toward the stone's full face is hidden, not the stone's.
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:glass'], [1, 0, 0, 'minecraft:stone']]), [6, 5]);
});

test('water is its fluid alone, blended, hiding its faces toward water and toward what holds water', async () => {
  assert.deepEqual(await draw([[0, 0, 0, WATER]]), [0, 6]);
  assert.deepEqual(await draw([[0, 0, 0, WATER], [1, 0, 0, WATER]]), [0, 10]);
  // Under stone its surface is drawn (lower than the block's top); beside
  // stone its side is not. The stone's faces toward water are all drawn.
  assert.deepEqual(await draw([[0, 0, 0, WATER], [0, 1, 0, 'minecraft:stone']]), [6, 6]);
  assert.deepEqual(await draw([[0, 0, 0, WATER], [1, 0, 0, 'minecraft:stone']]), [6, 5]);
});

test('a waterlogged stair is its model, opaque and drawn toward water, and its water, blended and hidden toward water', async () => {
  assert.deepEqual(await draw([[0, 0, 0, WET]]), [6, 6]);
  // Beside water: the stair's six faces, its water's five; the water's five.
  assert.deepEqual(await draw([[0, 0, 0, WET], [1, 0, 0, WATER]]), [6, 10]);
  // A stair that holds no water beside water: its six faces, the water's six.
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:oak_stairs[waterlogged=false]'], [1, 0, 0, WATER]]), [6, 6]);
});
