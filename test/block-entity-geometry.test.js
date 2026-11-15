'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const { test } = require('node:test');
const { MODULE } = require('../scripts/build-renderer');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');
const { bakedPack, bakedFile } = require('./helpers/community');

// The geometry see_build draws a block Minecraft draws through a block
// entity renderer with (Mesher.lone, web/viewer.js: its block model and the
// quads Minecraft's renderer draws, McbenchBlockEntities.java) is the live
// viewer's: for
// every such state of the data directory's bake (its render layers name the
// renderer; a waterlogged one also draws its water, which see_build draws as
// its fluid), the faces lone gives are the faces the live viewer's tile of a
// world holding that block alone holds: the same positions (the tile's less
// the block's place in it), texture coordinates, colours and textures. Run
// where the data directory holds a bake (MCBENCH_DATA_DIR); skipped
// elsewhere.
const PACK = bakedPack();
const LAYERS = bakedFile('render-layers.tsv');

// A drawn tile's triangles (renderer/src/prbm.rs write: the unindexed
// .prbm), each { values: its corners' positions less `offset` and uvs,
// key: its colour as bytes and texture }.
function tileTriangles(bytes, offset) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const vertices = bytes[2] | bytes[3] << 8 | bytes[4] << 16;
  let at = 8;
  const streams = {};
  for (let a = 0; a < (bytes[1] & 0x1f); a++) {
    let end = at;
    while (bytes[end] !== 0) end += 1;
    const name = Buffer.from(bytes.subarray(at, end)).toString();
    const flags = bytes[end + 1];
    at = Math.ceil((end + 2) / 4) * 4;
    const size = [0, 4, 0, 1, 2, 0, 4, 1, 2, 0, 4][flags & 0x0f];
    const count = (flags >> 4 & 3) + 1;
    streams[name] = { at, size, count };
    at += size * count * vertices;
  }
  at = Math.ceil(at / 4) * 4;
  const material = new Array(vertices);
  for (;; at += 12) {
    const index = view.getInt32(at, true);
    if (index === -1) break;
    const start = view.getInt32(at + 4, true);
    const count = view.getInt32(at + 8, true);
    for (let v = start; v < start + count; v++) material[v] = index;
  }
  const read = (name, v) => {
    const { at: base, size, count } = streams[name];
    return Array.from({ length: count }, (_, k) => (size === 4 ? view.getFloat32(base + (v * count + k) * 4, true) : bytes[base + v * count + k]));
  };
  const triangles = [];
  for (let v = 0; v < vertices; v += 3) {
    const values = [0, 1, 2].flatMap((k) => [...read('position', v + k).map((value, axis) => value - offset[axis]), ...read('uv', v + k)]);
    triangles.push({ values, key: `${read('color', v)} ${material[v]}` });
  }
  return triangles;
}

// Mesher.lone's faces in the same form, colours as the tile writes them.
function loneTriangles(faces) {
  const triangles = [];
  for (let face = 0; face < faces.count; face++) {
    const values = [0, 1, 2].flatMap((k) => [...faces.position.slice(face * 9 + k * 3, face * 9 + k * 3 + 3), ...faces.uv.slice(face * 6 + k * 2, face * 6 + k * 2 + 2)]);
    triangles.push({ values, key: `${Array.from(faces.color.slice(face * 3, face * 3 + 3), (value) => Math.round(value * 255))} ${faces.material[face]}` });
  }
  return triangles;
}

// Whether two lists of triangles are the same triangles, in any order, each
// value within a rounding of 32-bit floats (a tile's positions are the
// block's moved into the tile).
function same(a, b) {
  if (a.length !== b.length) return false;
  const left = [...b];
  return a.every((triangle) => {
    const at = left.findIndex((other) => other.key === triangle.key && other.values.every((value, i) => Math.abs(value - triangle.values[i]) < 1e-5));
    if (at < 0) return false;
    left.splice(at, 1);
    return true;
  });
}

test('every state drawn through a block entity renderer reaches see_build with the geometry and texture coordinates of the live viewer\'s tile', { skip: !PACK || !LAYERS }, async () => {
  const { Mesher } = await import('../web/mesher.mjs');
  const live = await Mesher.create(fs.readFileSync(MODULE), PACK);
  const alone = await Mesher.create(fs.readFileSync(MODULE), PACK);
  const states = LAYERS.toString('utf8').split('\n').filter((line) => line && !line.startsWith('layer\t'))
    .map((line) => line.split('\t')).filter(([name, , , renderer]) => renderer !== '-' && !name.includes('waterlogged=true')).map(([name]) => name);
  assert.ok(states.length > 0, `the bake draws ${states.length} states through block entity renderers`);
  let faces = 0;
  for (const name of states) {
    live.load(blocksBlueprint([[3, 70, 5, name]]), renderMask({ min: [3, 70, 5], max: [3, 70, 5] }), false);
    // The tile holding the block (renderer/src/mesh.rs: tiles are 32 blocks
    // from 2), whose positions are from its lowest corner, its heights the
    // world's.
    const [x, z] = [Math.floor((3 - 2) / 32), Math.floor((5 - 2) / 32)];
    const tile = tileTriangles(live.draw(x, z), [3 - (x * 32 + 2), 70, 5 - (z * 32 + 2)]);
    const { models, entities } = alone.lone(name, 0);
    const own = [...loneTriangles(models), ...loneTriangles(entities)];
    assert.ok(same(own, tile), `${name}: lone draws ${own.length} triangles, the live viewer's tile ${tile.length}, not the same`);
    faces += own.length;
  }
  assert.ok(faces > 0);
});
