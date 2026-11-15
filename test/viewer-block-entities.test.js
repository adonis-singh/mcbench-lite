'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');
const { MODULE } = require('../scripts/build-renderer');
const { testBlockStates } = require('./helpers/block-table');
const { testBlockTints } = require('./helpers/block-tints');
const { mesherPack } = require('./helpers/mesher-pack');
const { viewerHarness } = require('./helpers/viewer');

// see_build's viewer (web/viewer.js) draws a block Minecraft's client draws
// through a block entity renderer with the live viewer's own geometry: the
// faces the live viewer's mesher module draws of it alone, over the same
// pack (Mesher.lone), placed, culled as see_build culls and textured from
// the atlas by the map's texture names. Here the lamp of the test pack
// (helpers/mesher-pack.js: a cube whose face toward each direction has the
// texture of that direction's index, culled toward its own side) is such a
// block, with no block model of its own; and so is the shrine: that cube,
// and a red quad of texture 6 in its middle as a block entity renderer's.
// Stone is a plain full cube.
const ALL = 63;
const LAMP = 'minecraft:lamp[lit=false]';
const SHRINE = 'minecraft:shrine';
const STATES = testBlockStates([
  { name: 'minecraft:stone', full: ALL, lightBlock: 15 },
  { name: LAMP, renderer: 'minecraft:lamp' },
  { name: SHRINE, renderer: 'minecraft:shrine' },
]);
const TINTS = testBlockTints(STATES);
// The map's textures, by the pack's texture index (the face's direction, and the shrine's quad).
const TEXTURES = ['down', 'up', 'north', 'south', 'west', 'east', 'quad'].map((side) => `minecraft:block/lamp_${side}`);
const ATLAS = Object.fromEntries(TEXTURES.map((name, index) => [name.slice('minecraft:'.length), [(index % 2) * 16, Math.floor(index / 2) * 16, 16, 16]]));
const cube = (texture) => ({
  textures: { all: texture },
  elements: [{ from: [0, 0, 0], to: [16, 16, 16], faces: Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((face) => [face, { texture: '#all', cullface: face }])) }],
});
const ASSETS = {
  'blocks.json': { stone: [{}, {}], lamp: [{ lit: ['false', 'true'] }, { lit: 'false' }], shrine: [{}, {}] },
  'block_definitions.json': { stone: { variants: { '': { model: 'minecraft:block/stone' } } } },
  'models.json': { 'block/stone': cube('minecraft:block/stone') },
  'atlas_map.json': { 'block/stone': [0, 0, 16, 16], ...ATLAS },
};
const WASM = fs.readFileSync(MODULE);
const PACK = mesherPack();

let open;
const fetched = [];
test.before(async () => {
  open = await viewerHarness(STATES, ASSETS, TINTS, { wasm: WASM, pack: PACK, textures: TEXTURES }, fetched);
});

// The quads one world draws: by quad, its four corners' positions, texture
// coordinates and brightness (the largest channel of each corner's colour).
async function draw(blocks) {
  const { viewer, drawn } = await open();
  const names = [...new Set(blocks.map(([, , , name]) => name))];
  const palette = names.map((name) => {
    const [id, properties] = name.split('[');
    return { id, states: properties ? Object.fromEntries(properties.slice(0, -1).split(',').map((pair) => pair.split('='))) : {}, state: STATES.ids.get(name) };
  });
  viewer.applyDelta({ chunks: [{ key: 'a', version: 1, origin: [0, 64, 0], palette, blocks: blocks.map(([x, y, z, name]) => [x, y, z, names.indexOf(name)]) }], removed: [] });
  await viewer.prepare();
  viewer.flush();
  const quads = [];
  for (const [position, color, texture] of drawn()) {
    for (let q = 0; q < position.length / 12; q++) {
      quads.push([0, 1, 2, 3].map((i) => ({
        position: position.slice(q * 12 + i * 3, q * 12 + i * 3 + 3),
        texture: texture.slice(q * 8 + i * 2, q * 8 + i * 2 + 2),
        brightness: Math.max(...color.slice(q * 12 + i * 3, q * 12 + i * 3 + 3)),
      })));
    }
  }
  return quads;
}

// The quads the live viewer's mesher draws of the lamp alone at `at`, faces
// with a cullface toward `culled` hidden, as see_build places and textures
// them: each corner's position and atlas texture coordinates.
async function lone(name, at, culled) {
  const { Mesher } = await import('../web/mesher.mjs');
  const mesher = await Mesher.create(WASM, PACK);
  const { models, entities } = mesher.lone(name, culled);
  const quads = [];
  for (const faces of [models, entities]) for (let q = 0; q < faces.count / 2; q++) {
    const [x, y, w, h] = ATLAS[TEXTURES[faces.material[q * 2]].slice('minecraft:'.length)];
    // The atlas the harness draws is 32 texels on its longest side.
    const size = 32;
    quads.push([[0, 0], [0, 1], [0, 2], [1, 2]].map(([half, k]) => {
      const face = q * 2 + half;
      return {
        position: [0, 1, 2].map((axis) => Math.fround(faces.position[face * 9 + k * 3 + axis] + at[axis])),
        texture: [Math.fround((x + faces.uv[face * 6 + k * 2] * w) / size), Math.fround((y + faces.uv[face * 6 + k * 2 + 1] * h) / size)],
      };
    }));
  }
  return quads;
}

const close = (a, b) => a.length === b.length && a.every((value, i) => Math.abs(value - b[i]) < 1e-6);

test('a world without a block drawn through a block entity renderer never loads the live viewer\'s mesher', async () => {
  fetched.length = 0;
  assert.equal((await draw([[0, 0, 0, 'minecraft:stone']])).length, 6);
  assert.ok(!fetched.some((url) => url.startsWith('/live/') || url === '/api/minecraft/status'), `fetched ${fetched}`);
});

test('a block drawn through a block entity renderer is the live viewer\'s faces of it, its model\'s and its renderer\'s, placed and textured from the atlas by name', async () => {
  const drawn = await draw([[2, 3, 4, SHRINE]]);
  const expected = await lone(SHRINE, [2, 67, 4], 0);
  assert.equal(drawn.length, 7);
  assert.equal(drawn.length, expected.length);
  for (const [q, quad] of expected.entries()) {
    quad.forEach((corner, i) => {
      assert.ok(close(drawn[q][i].position, corner.position), `quad ${q} corner ${i} at ${drawn[q][i].position}, the module's ${corner.position}`);
      assert.ok(close(drawn[q][i].texture, corner.texture), `quad ${q} corner ${i} textured ${drawn[q][i].texture}, the module's ${corner.texture}`);
    });
  }
});

test('its model\'s faces are culled as see_build culls them, and its renderer\'s quad is lit by its own cell, unshaded', async () => {
  // Stone east of the shrine and above it.
  const drawn = await draw([[0, 0, 0, SHRINE], [1, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:stone']]);
  // Up and east (bits 1 and 5 over down, up, north, south, west, east) are
  // hidden: the shrine is its cube's other four faces and its quad, and the
  // stones' twelve faces are drawn beside them.
  const expected = await lone(SHRINE, [0, 64, 0], (1 << 1) | (1 << 5));
  assert.equal(expected.length, 5);
  assert.equal(drawn.length, 5 + 12);
  for (const quad of expected) {
    assert.ok(drawn.some((other) => other.every((corner, i) => close(corner.position, quad[i].position) && close(corner.texture, quad[i].texture))), `the shrine's quad at ${quad[0].position} is drawn`);
  }
  const middle = drawn.filter((quad) => quad.every((corner) => corner.position[2] === 0.5));
  assert.equal(middle.length, 1);
  // Its cell is lit by the sky through its open sides: every corner alike.
  const brightness = new Set(middle[0].map((corner) => corner.brightness.toFixed(4)));
  assert.equal(brightness.size, 1, `the quad's corners are ${[...brightness]}`);
});
