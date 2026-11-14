'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { testBlockStates } = require('./helpers/block-table');
const { testBlockTints } = require('./helpers/block-tints');
const { viewerHarness } = require('./helpers/viewer');

// The see_build viewer (web/viewer.js) with deepslate itself, over a stand-in
// WebGL context that keeps what each buffer was given, and the assets of two
// blocks: what reset() lets go, and that what is drawn after it is what a page
// just loaded draws.

// The block table of the assets' two blocks, its facts written for the test:
// cubes with six full faces, one of which gives light.
const STATES = testBlockStates([
  { name: 'minecraft:stone', full: 63, lightBlock: 15 },
  { name: 'minecraft:glowstone', full: 63, lightBlock: 15, emission: 15 },
]);
const cube = (texture) => ({
  textures: { all: texture },
  elements: [{
    from: [0, 0, 0],
    to: [16, 16, 16],
    faces: Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((face) => [face, { texture: '#all', cullface: face }])),
  }],
});
const ASSETS = {
  'blocks.json': { stone: [{}, {}], glowstone: [{}, {}] },
  'block_definitions.json': {
    stone: { variants: { '': { model: 'minecraft:block/stone' } } },
    glowstone: { variants: { '': { model: 'minecraft:block/glowstone' } } },
  },
  'models.json': { 'block/stone': cube('minecraft:block/stone'), 'block/glowstone': cube('minecraft:block/glowstone') },
  'atlas_map.json': { 'block/stone': [0, 0, 16, 16], 'block/glowstone': [16, 0, 16, 16] },
};

let open;
test.before(async () => {
  open = await viewerHarness(STATES, ASSETS, testBlockTints(STATES));
});

// A server chunk: `blocks` as [x, y, z, id] within the chunk at `origin`.
function chunk(key, version, origin, blocks) {
  const ids = [...new Set(blocks.map(([, , , id]) => id))];
  return { key, version, origin, palette: ids.map((id) => ({ id: `minecraft:${id}`, state: STATES.idOf(`minecraft:${id}`, {}) })), blocks: blocks.map(([x, y, z, id]) => [x, y, z, ids.indexOf(id)]) };
}
const slab = (id, y) => Array.from({ length: 16 }, (_, i) => [i, y, 3, id]);
// Two chats' worlds, sharing one chunk position: a lit room, and a wall across two chunks.
const FIRST = [
  chunk('a', 1, [0, 64, 0], [...slab('stone', 0), ...slab('stone', 4), [5, 2, 3, 'glowstone'], [0, 1, 3, 'stone'], [15, 1, 3, 'stone']]),
  chunk('b', 1, [16, 64, 0], [...slab('stone', 0), [0, 1, 3, 'stone']]),
  chunk('c', 1, [0, 80, 16], slab('stone', 15)),
];
const SECOND = [
  chunk('a', 7, [0, 64, 0], [...slab('stone', 2), [15, 3, 3, 'stone'], [8, 3, 3, 'glowstone']]),
  chunk('d', 7, [-16, 64, 0], [...slab('stone', 2), [15, 3, 3, 'stone']]),
];

function load(viewer, chunks) {
  for (const each of chunks) viewer.applyDelta({ chunks: [each], removed: [] });
  viewer.flush();
}

test('reset lets the whole world go: its chunks\' arrays, their meshes and their buffers', async () => {
  const v = await open();
  const idle = v.buffers.size;
  load(v.viewer, FIRST);
  assert.equal(v.world.size, 16 + 16 + 3 + 16 + 1 + 16);
  assert.ok(v.world.cells.size >= 3 && v.chunks().length >= 3 && v.buffers.size > idle);
  // The glowstone gives light: its light class's emission, from the table.
  const rule = (await import('../web/light.mjs')).lightRule(STATES);
  assert.deepEqual(v.world.getBlocks().filter((placed) => rule.emission[placed.light] > 0).map((placed) => rule.emission[placed.light]), [15]);
  // A clip box and back: a version kept aside, with its meshes.
  v.viewer.applyDelta({ chunks: [], removed: ['b'] });
  v.viewer.flush();
  v.viewer.reset();
  assert.equal(v.world.cells.size, 0, 'no chunk\'s array of blocks is kept');
  assert.equal(v.world.chunks.size, 0);
  assert.equal(v.world.size, 0);
  assert.equal(v.world.at(5, 66, 3), undefined, 'nor the chunk last looked at');
  assert.equal(v.chunks().length, 0, 'deepslate holds no chunk');
  assert.equal(v.buffers.size, idle, 'every mesh\'s buffers are deleted');
  assert.equal(v.viewer.pending(), 0, 'nothing of the old world waits to be meshed');
  assert.deepEqual(v.drawn(), []);
});

test('a world loaded after a reset draws as it does on a page just loaded', async () => {
  const fresh = await open();
  load(fresh.viewer, SECOND);
  const expected = fresh.drawn();
  assert.ok(expected.length >= 2 && expected.every((mesh) => mesh[0].length > 0));

  const reused = await open();
  load(reused.viewer, FIRST);
  assert.notDeepEqual(reused.drawn(), expected);
  reused.viewer.reset();
  load(reused.viewer, SECOND);
  assert.deepEqual(reused.drawn(), expected);
  // And an edit after it, as after a first load: a chunk replaced, one removed.
  const edit = (viewer) => {
    viewer.applyDelta({ chunks: [chunk('a', 8, [0, 64, 0], [...slab('stone', 2), [8, 3, 3, 'stone']])], removed: ['d'] });
    viewer.flush();
  };
  edit(fresh.viewer);
  edit(reused.viewer);
  assert.deepEqual(reused.drawn(), fresh.drawn());
  assert.equal(reused.world.size, fresh.world.size);
});
