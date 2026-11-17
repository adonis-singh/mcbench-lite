'use strict';

// see_build's pictures from the real mcbench-draw, through the server's
// client (server/shot.js), of a world the real engine settled
// (server/shot-world.js), with the assets of the data folder MCBENCH_DATA_DIR
// names (the viewer's bake, the atlas, Paper's block table) and the renderer
// of the tools folder. No model is called and no saved build is touched.
// Run it with the data folder of the server MCBENCH_UI_URL names, as
// npm run test:ui does. The pictures are drawing's own tests' (native/draw):
// here they are JPEGs of the asked sizes, in the asked order, and a world is
// sent to the helper once.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { World } = require('../server/world');

// A JPEG's size, from its first start-of-frame segment.
function jpegSize(bytes) {
  for (let at = 2; at < bytes.length;) {
    const marker = bytes[at + 1];
    const length = bytes.readUInt16BE(at + 2);
    if (marker >= 0xc0 && marker <= 0xc3) return [bytes.readUInt16BE(at + 7), bytes.readUInt16BE(at + 5)];
    at += 2 + length;
  }
  throw new Error('no frame in the JPEG');
}

test('the real renderer draws resolved shots as JPEGs of their sizes and in their order, a world it keeps once, and an edit of it', {
  skip: process.env.MCBENCH_UI_INTEGRATION !== '1',
  timeout: 300_000,
}, async (t) => {
  const { settle, stopShotEngines } = require('../server/shot-world');
  const shot = require('../server/shot');
  t.after(async () => { await shot.closeRenderer(); await stopShotEngines(); });
  const world = new World();
  for (let x = 0; x < 6; x++) for (let z = 0; z < 4; z++) world.set(x, 0, z, 'minecraft:stone');
  world.set(1, 1, 1, 'minecraft:glass');
  world.set(3, 1, 1, 'minecraft:water', { level: '0' });
  world.set(4, 1, 2, 'minecraft:oak_stairs', { facing: 'east', half: 'bottom', shape: 'straight', waterlogged: 'true' });
  world.set(0, 1, 3, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  world.finalize();
  const conversation = { id: 'shot-draw-integration', world };
  const settled = (await settle(conversation.id, world, [0, 5])).get(0);
  const later = (await settle(conversation.id, world, [5])).get(5);
  const resolve = (spec, from = settled) => ({ ...shot.resolveShot(spec, from.world, from.entities), tick: from.ticks });
  const shots = [
    resolve({ view: 'iso_se' }),
    resolve({ view: 'iso_nw', size: 'small' }),
    resolve({ view: 'top', projection: 'orthographic', grid: true }),
    resolve({ view: 'front', clip: { from: [0, 0, 0], to: [2, 1, 3] } }),
    resolve({ view: 'iso_sw' }, later),
  ];
  const timings = {};
  const pictures = await shot.renderShots(conversation.id, shots, undefined, timings);
  assert.deepEqual(pictures.map(jpegSize), [[2184, 1640], [1092, 820], [2184, 1640], [2184, 1640], [2184, 1640]]);
  assert.ok(timings['world loading'] > 0, 'the worlds were sent');
  const again = {};
  const repeat = await shot.renderShots(conversation.id, shots.slice(0, 1), undefined, again);
  assert.equal(again['world loading'], undefined, 'a world the helper keeps is not sent again');
  assert.deepEqual(jpegSize(repeat[0]), [2184, 1640]);

  // An edit: the next revision of the build, sent with the previous world
  // as its base (the helper re-meshes what changed), drawn as asked.
  const edited = new World();
  world.forEach((x, y, z, entry) => edited.set(x, y, z, entry.id, entry.states));
  edited.set(5, 1, 3, 'minecraft:gold_block');
  edited.finalize();
  const next = (await settle(conversation.id, edited, [0])).get(0);
  const afterEdit = {};
  const editedPictures = await shot.renderShots(conversation.id, [{ ...shot.resolveShot({ view: 'iso_se' }, next.world, next.entities), tick: 0 }], undefined, afterEdit);
  assert.ok(afterEdit['world loading'] > 0, 'the edited world was sent');
  assert.deepEqual(editedPictures.map(jpegSize), [[2184, 1640]]);
});
