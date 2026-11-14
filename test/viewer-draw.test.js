'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { testBlockStates } = require('./helpers/block-table');
const { testBlockTints } = require('./helpers/block-tints');
const { viewerHarness } = require('./helpers/viewer');

// What see_build's viewer (web/viewer.js) draws of a block, and in which
// pass, by the block table's facts written for the test: glass that skips
// rendering beside glass and is drawn blended; water (a fluid alone, drawn
// blended, skipping beside every state that holds water); a stair that holds
// water and one that does not; stone; leaves whose top is tinted, and
// petals whose top is of tint index 1 and north side of 0, by colours
// written for the test. Every model is a cube whose faces cull toward their
// own sides, so a block's quads are its faces drawn.
const ALL = 63;
const WATER = 'minecraft:water[level=0]';
const WET = 'minecraft:oak_stairs[waterlogged=true]';
const STATES = testBlockStates([
  { name: 'minecraft:stone', full: ALL, lightBlock: 15 },
  { name: 'minecraft:glass', blended: true },
  { name: WATER, model: false, fluid: WATER, fluidBlended: true, lightBlock: 1 },
  { name: 'minecraft:oak_stairs[waterlogged=false]' },
  { name: WET, fluid: WATER, fluidBlended: true, lightBlock: 1 },
  { name: 'minecraft:oak_leaves' },
  { name: 'minecraft:pink_petals' },
], {
  skips: [['minecraft:glass', 'minecraft:glass', ALL], [WATER, WATER, ALL], [WATER, WET, ALL]],
});
const TINTS = testBlockTints(STATES, {
  [WATER]: { world: [0x3f76e4, 0x3f76e4] },
  'minecraft:oak_leaves': { world: [0x77ab2f, 0x77ab2f], faces: '0' },
  'minecraft:pink_petals': { world: [0xffffff, 0x91bd59], faces: '1' },
});
const cube = (texture, tints = {}) => ({
  textures: { all: texture },
  elements: [{
    from: [0, 0, 0],
    to: [16, 16, 16],
    faces: Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((face) => [face, { texture: '#all', cullface: face, ...(face in tints ? { tintindex: tints[face] } : {}) }])),
  }],
});
const ASSETS = {
  'blocks.json': {
    stone: [{}, {}], glass: [{}, {}], water: [{ level: ['0'] }, { level: '0' }], oak_stairs: [{ waterlogged: ['false', 'true'] }, { waterlogged: 'false' }],
    oak_leaves: [{}, {}], pink_petals: [{}, {}],
  },
  'block_definitions.json': {
    stone: { variants: { '': { model: 'minecraft:block/stone' } } },
    glass: { variants: { '': { model: 'minecraft:block/glass' } } },
    oak_stairs: { variants: { '': { model: 'minecraft:block/oak_stairs' } } },
    oak_leaves: { variants: { '': { model: 'minecraft:block/oak_leaves' } } },
    pink_petals: { variants: { '': { model: 'minecraft:block/pink_petals' } } },
  },
  'models.json': {
    'block/stone': cube('minecraft:block/stone'), 'block/glass': cube('minecraft:block/glass'), 'block/oak_stairs': cube('minecraft:block/oak_planks'),
    'block/oak_leaves': cube('minecraft:block/stone', { up: 0 }), 'block/pink_petals': cube('minecraft:block/stone', { up: 1, north: 0 }),
  },
  'atlas_map.json': {
    'block/stone': [0, 0, 16, 16], 'block/glass': [16, 0, 16, 16], 'block/oak_planks': [0, 0, 16, 16],
    'block/water_still': [16, 0, 16, 16], 'block/water_flow': [16, 0, 16, 16],
  },
};

let open;
test.before(async () => {
  open = await viewerHarness(STATES, ASSETS, TINTS);
});

// The quads drawn of one world, by pass: [opaque, blended]; with `colors`,
// the colours of the vertices drawn, each over its largest channel (the
// viewer's light scales all three alike), sorted.
async function draw(blocks, colors = false) {
  const { viewer, chunks, drawn } = await open();
  const names = [...new Set(blocks.map(([, , , name]) => name))];
  const palette = names.map((name) => {
    const [id, properties] = name.split('[');
    return { id, states: properties ? Object.fromEntries(properties.slice(0, -1).split(',').map((pair) => pair.split('='))) : {}, state: STATES.ids.get(name) };
  });
  viewer.applyDelta({ chunks: [{ key: 'a', version: 1, origin: [0, 64, 0], palette, blocks: blocks.map(([x, y, z, name]) => [x, y, z, names.indexOf(name)]) }], removed: [] });
  viewer.flush();
  if (colors) {
    const seen = new Set();
    for (const [, color] of drawn()) {
      for (let at = 0; at < color.length; at += 3) {
        const top = Math.max(color[at], color[at + 1], color[at + 2]);
        seen.add([0, 1, 2].map((channel) => (color[at + channel] / top).toFixed(3)).join());
      }
    }
    return [...seen].sort();
  }
  const count = (mesh) => chunks().reduce((sum, chunk) => sum + chunk[mesh].quadVertices() / 4, 0);
  return [count('mesh'), count('transparentMesh')];
}

// A colour over its largest channel, as draw gives it.
const over = (color) => {
  const channels = [color >> 16 & 255, color >> 8 & 255, color & 255];
  return channels.map((channel) => (channel / Math.max(...channels)).toFixed(3)).join();
};

test('a face of a tint index takes the colour Minecraft gives its state for that index, and a fluid its fluid\'s', async () => {
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:stone']], true), [over(0xffffff)]);
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:oak_leaves']], true), [over(0xffffff), over(0x77ab2f)].sort());
  // Index 1 is green, index 0 white.
  assert.deepEqual(await draw([[0, 0, 0, 'minecraft:pink_petals']], true), [over(0xffffff), over(0x91bd59)].sort());
  assert.deepEqual(await draw([[0, 0, 0, WATER]], true), [over(0x3f76e4)]);
  assert.deepEqual(await draw([[0, 0, 0, WET]], true), [over(0xffffff), over(0x3f76e4)].sort(), 'the stair white, its water blue');
});

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
