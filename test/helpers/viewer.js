'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// The see_build viewer (web/viewer.js) with deepslate itself, over a stand-in
// WebGL context that keeps what each buffer was given, reading `states`
// (a BlockStates) and the page's `assets` (by file name under /api/assets/).
const ROOT = path.join(__dirname, '..', '..');

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

// Loads the viewer into this process (once per test file); resolves with
// open(), which makes a viewer and gives it with what it holds: the world,
// deepslate's chunks, what a frame draws, and the buffers alive.
async function viewerHarness(states, assets) {
  let captured;
  const deepslate = require(path.join(ROOT, 'node_modules', 'deepslate', 'dist', 'deepslate.umd.cjs'));
  // The renderer and the world it draws, as the viewer makes them.
  class StructureRenderer extends deepslate.StructureRenderer {
    constructor(gl, world, ...rest) {
      super(gl, world, ...rest);
      captured = { renderer: this, world };
    }
  }
  globalThis.deepslate = { ...deepslate, StructureRenderer };
  globalThis.fetch = async (url) => (url === '/api/block-states'
    ? { arrayBuffer: async () => { const bytes = states.bytes(); return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length); } }
    : { json: async () => assets[url.slice('/api/assets/'.length)] });
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
  globalThis.__viewer = { light: await import('../../web/light.mjs'), glMatrix: require('gl-matrix') };
  const source = fs.readFileSync(path.join(ROOT, 'web', 'viewer.js'), 'utf8')
    .replace(/^import (\{.*\}) from '\.\/light\.mjs';$/m, 'const $1 = globalThis.__viewer.light;')
    .replace(/^import (\{.*\}) from '\/vendor\/gl-matrix\/index\.js';$/m, 'const $1 = globalThis.__viewer.glMatrix;')
    .replace('export async function createViewer', 'globalThis.__viewer.createViewer = async function createViewer');
  vm.runInThisContext(source, { filename: 'viewer.js' });
  const { createViewer } = globalThis.__viewer;
  return async function open() {
    const canvas = { width: 640, height: 480, clientWidth: 640, clientHeight: 480 };
    const { gl, buffers } = fakeGl(canvas);
    canvas.getContext = () => gl;
    const viewer = await createViewer(canvas, -64);
    const { renderer, world } = captured;
    const chunks = () => renderer.chunkBuilder.chunks.flat(2).filter(Boolean);
    // What a frame draws: each mesh's buffers, in the order deepslate draws them.
    const drawn = () => renderer.chunkBuilder.getMeshes().map((mesh) => ['posBuffer', 'colorBuffer', 'textureBuffer', 'textureLimitBuffer', 'normalBuffer', 'indexBuffer']
      .map((field) => mesh[field].data));
    return { viewer, world, chunks, drawn, buffers };
  };
}

module.exports = { viewerHarness };
