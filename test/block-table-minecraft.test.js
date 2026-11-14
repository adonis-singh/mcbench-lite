'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const { minecraftLive } = require('../server/minecraft-live');
const { PAPER } = require('../server/paper-files');
const { JAVA } = require('../server/tools');
const { BlockStates } = require('../server/block-states');

// What the pinned Minecraft says of block states, in the block table
// minecraftLive.blockTable() makes from Paper's own classes. It runs where
// that table can be made without a download: the data folder
// (MCBENCH_DATA_DIR) holds Paper laid out (its jar and its versions folder)
// and the tools folder (MCBENCH_TOOLS_DIR) the JDK.
const ready = fs.existsSync(path.join(minecraftLive.paperDir, PAPER[minecraftLive.minecraftVersion].jar))
  && fs.existsSync(path.join(minecraftLive.paperDir, 'versions')) && fs.existsSync(JAVA);

test('the block table holds Minecraft\'s light, faces and face shape pairs', { skip: !ready && 'needs Paper laid out in the data folder and the JDK in the tools folder', timeout: 300000 }, async () => {
  const states = new BlockStates(fs.readFileSync(await minecraftLive.blockTable(), 'utf8'));
  const fact = (id, properties = {}) => {
    const state = states.idOf(id, properties);
    return [states.emission[state], states.lightBlock[state], states.fullFaces[state]];
  };
  assert.equal(states.idOf('minecraft:air', {}), 0);
  // The invisible light block gives its level and dims nothing, unless it holds water.
  for (let level = 0; level <= 15; level++) {
    assert.deepEqual(fact('minecraft:light', { level: String(level) }), [level, 0, 0]);
    assert.deepEqual(fact('minecraft:light', { level: String(level), waterlogged: 'true' }), [level, 1, 0]);
  }
  // Sea pickles glow only in water, brighter the more there are.
  for (let pickles = 1; pickles <= 4; pickles++) {
    assert.deepEqual(fact('minecraft:sea_pickle', { pickles: String(pickles), waterlogged: 'true' }), [3 + 3 * pickles, 1, 0]);
    assert.deepEqual(fact('minecraft:sea_pickle', { pickles: String(pickles), waterlogged: 'false' }), [0, 0, 0]);
  }
  for (const [id, level] of [['minecraft:redstone_lamp', 15], ['minecraft:furnace', 13], ['minecraft:copper_bulb', 15]]) {
    assert.deepEqual(fact(id, { lit: 'true' }), [level, 15, 63], id);
    assert.deepEqual(fact(id, { lit: 'false' }), [0, 15, 63], id);
  }
  assert.deepEqual([fact('minecraft:candle_cake', { lit: 'true' }), fact('minecraft:candle_cake', { lit: 'false' })], [[3, 0, 0], [0, 0, 0]]);
  assert.deepEqual([0, 1, 2, 3, 4].map((charges) => fact('minecraft:respawn_anchor', { charges: String(charges) })[0]), [0, 3, 7, 11, 15]);
  // Faces as bits over down, up, north, south, west, east.
  assert.deepEqual(fact('minecraft:stone'), [0, 15, 63]);
  assert.deepEqual(fact('minecraft:water'), [0, 1, 0]);
  assert.deepEqual(fact('minecraft:glass'), [0, 0, 0]);
  assert.deepEqual(fact('minecraft:stone_slab', { type: 'bottom' }), [0, 0, 1]);
  assert.deepEqual(fact('minecraft:stone_slab', { type: 'top' }), [0, 0, 2]);
  assert.deepEqual(fact('minecraft:stone_slab', { type: 'double' }), [0, 15, 63]);
  assert.deepEqual(fact('minecraft:oak_stairs', { facing: 'north', half: 'bottom' }), [0, 0, 1 | 4]);
  // Face shapes: a bottom slab's half side and a top slab's close the face
  // between them, two bottom slabs' do not; its full bottom closes alone.
  const { faces, occludes, shapes } = states;
  const face = (id, properties, direction) => faces[states.idOf(id, properties) * 6 + direction];
  const closes = (a, b) => occludes[a * shapes + b] === 1;
  const bottomEast = face('minecraft:stone_slab', { type: 'bottom' }, 5);
  assert.ok(closes(bottomEast, face('minecraft:stone_slab', { type: 'top' }, 4)));
  assert.ok(!closes(bottomEast, face('minecraft:stone_slab', { type: 'bottom' }, 4)));
  assert.ok(closes(face('minecraft:stone_slab', { type: 'bottom' }, 0), 0));
  // Shape 0 is the empty face; the full face is the one shape that closes beside it.
  const full = [...Array(shapes).keys()].filter((shape) => closes(0, shape));
  assert.equal(full.length, 1);
  let partialPairs = 0;
  for (let a = 1; a < shapes; a++) for (let b = 1; b < shapes; b++) if (a !== full[0] && b !== full[0] && closes(a, b)) partialPairs += 1;
  assert.equal(partialPairs, 300);
});
