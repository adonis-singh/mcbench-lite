'use strict';

// A capture as server/McbenchEntities.java writes it (mcbench-entities.json),
// made here for tests of the page's entity drawing (web/entity-draw.mjs):
// textures, every quad's shape kept once (its corners, then its texture
// coordinates), meshes (a quad's shape, texture and colour each), poses
// kept once, and the kinds given.
//
//   entityCapture({ textures, meshes: [[{ corners, uv, texture, color }]], poses: [[16]], kinds, items, specials })

function base64(array) {
  return Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString('base64');
}

function entityCapture({ textures, meshes, poses, kinds = {}, items = {}, specials = {}, atlas = null, id = 1 }) {
  const shapes = [];
  const quads = [];
  const offsets = [0];
  for (const mesh of meshes) {
    for (const quad of mesh) {
      shapes.push(...quad.corners, ...quad.uv);
      quads.push(shapes.length / 20 - 1, textures.indexOf(quad.texture), quad.color ?? -1);
    }
    offsets.push(quads.length / 3);
  }
  return {
    version: 1,
    id,
    atlas: atlas ?? { width: 64, height: 64, rects: Object.fromEntries(textures.map((texture, i) => [texture, [i * 16, 0, 16, 16]])) },
    textures,
    shapes: base64(new Float32Array(shapes)),
    quads: base64(new Int32Array(quads)),
    meshes: base64(new Int32Array(offsets)),
    poses: base64(new Float32Array(poses.flat())),
    kinds,
    items,
    specials,
  };
}

// A unit square in the x y plane at z = 0, its texture all of `texture`.
const square = (texture, color = -1) => ({ corners: [0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0], uv: [0, 0, 1, 0, 1, 1, 0, 1], texture, color });

const IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const translation = (x, y, z) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];
const levels = () => Array.from({ length: 16 }, (_, i) => i);

module.exports = { entityCapture, square, IDENTITY, translation, levels };
