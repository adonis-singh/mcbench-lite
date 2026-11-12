'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

// The see_build viewer (web/viewer.js) with deepslate itself, over a stand-in
// WebGL context that keeps what each buffer was given, and the assets of one
// block: what reset() lets go, and that what is drawn after it is what a page
// just loaded draws.
const ROOT = path.join(__dirname, '..');
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

// A WebGL context that does nothing but keep each buffer's data.
function fakeGl(canvas) {
  const buffers = new Set();
  const bound = new Map();
  const own = {
    canvas,
    createBuffer() { const buffer = { data: null }; buffers.add(buffer); return buffer; },
    deleteBuffer(buffer) { buffers.delete(buffer); },
    bindBuffer(target, buffer) { bound.set(target, buffer); },
    bufferData(target, data) { bound.get(target).data = Array.from(data); },
    getShaderParameter: () => true,
    getProgramParameter: () => true,
    getAttribLocation: () => 0,
    getParameter: () => 4096,
    isContextLost: () => false,
  };
  const gl = new Proxy(own, {
    get(target, name) {
      if (name in target) return target[name];
      // A constant, or a call that answers with an object of its own.
      if (/^[A-Z0-9_]+$/.test(name)) return name.length;
      return () => ({});
    },
  });
  return { gl, buffers };
}

let createViewer;
let captured;
test.before(async () => {
  const deepslate = require(path.join(ROOT, 'node_modules', 'deepslate', 'dist', 'deepslate.umd.cjs'));
  // The renderer and the world it draws, as the viewer makes them.
  class StructureRenderer extends deepslate.StructureRenderer {
    constructor(gl, world, ...rest) {
      super(gl, world, ...rest);
      captured = { renderer: this, world };
    }
  }
  globalThis.deepslate = { ...deepslate, StructureRenderer };
  globalThis.fetch = async (url) => ({ json: async () => ASSETS[url.slice('/api/assets/'.length)] });
  globalThis.Image = class {
    constructor() { this.width = 32; this.height = 16; }
    set src(_) { queueMicrotask(() => this.onload()); }
  };
  globalThis.document = {
    createElement: () => ({
      getContext: () => ({
        drawImage() {},
        getImageData: (x, y, width, height) => ({ data: new Uint8ClampedArray(width * height * 4), width, height }),
      }),
    }),
  };
  globalThis.__viewer = { light: await import('../web/light.mjs'), glMatrix: require('gl-matrix') };
  const source = fs.readFileSync(path.join(ROOT, 'web', 'viewer.js'), 'utf8')
    .replace(/^import (\{.*\}) from '\.\/light\.mjs';$/m, 'const $1 = globalThis.__viewer.light;')
    .replace(/^import (\{.*\}) from '\/vendor\/gl-matrix\/index\.js';$/m, 'const $1 = globalThis.__viewer.glMatrix;')
    .replace('export async function createViewer', 'globalThis.__viewer.createViewer = async function createViewer');
  vm.runInThisContext(source, { filename: 'viewer.js' });
  createViewer = globalThis.__viewer.createViewer;
});

// A viewer, with what it holds: the world, deepslate's chunks, the buffers alive.
async function open() {
  const canvas = { width: 640, height: 480, clientWidth: 640, clientHeight: 480 };
  const { gl, buffers } = fakeGl(canvas);
  canvas.getContext = () => gl;
  const viewer = await createViewer(canvas);
  const { renderer, world } = captured;
  const chunks = () => renderer.chunkBuilder.chunks.flat(2).filter(Boolean);
  // What a frame draws: each mesh's buffers, in the order deepslate draws them.
  const drawn = () => renderer.chunkBuilder.getMeshes().map((mesh) => ['posBuffer', 'colorBuffer', 'textureBuffer', 'textureLimitBuffer', 'normalBuffer', 'indexBuffer']
    .map((field) => mesh[field].data));
  return { viewer, world, chunks, drawn, buffers };
}

// A server chunk: `blocks` as [x, y, z, id] within the chunk at `origin`.
function chunk(key, version, origin, blocks) {
  const ids = [...new Set(blocks.map(([, , , id]) => id))];
  return { key, version, origin, palette: ids.map((id) => ({ id: `minecraft:${id}` })), blocks: blocks.map(([x, y, z, id]) => [x, y, z, ids.indexOf(id)]) };
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
  assert.ok(v.world.cells.size >= 3 && v.chunks().length >= 3 && v.buffers.size > idle && v.world.emitters.size === 1);
  // A clip box and back: a version kept aside, with its meshes.
  v.viewer.applyDelta({ chunks: [], removed: ['b'] });
  v.viewer.flush();
  v.viewer.reset();
  assert.equal(v.world.cells.size, 0, 'no chunk\'s array of blocks is kept');
  assert.equal(v.world.chunks.size, 0);
  assert.equal(v.world.emitters.size, 0);
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
