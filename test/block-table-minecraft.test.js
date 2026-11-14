'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const { minecraftLive } = require('../server/minecraft-live');
const { PAPER } = require('../server/paper-files');
const { JAVA } = require('../server/tools');
const { BlockStates } = require('../server/block-states');
const { opaqueStates } = require('./helpers/block-table');

// What the pinned Minecraft says of block states, in the block table
// minecraftLive.blockTable() makes from Paper's own classes, and in the
// render layers the viewer's bake reads from its client jar. The table's
// tests run where it can be made without a download: the data folder
// (MCBENCH_DATA_DIR) holds Paper laid out (its jar and its versions folder)
// and the tools folder (MCBENCH_TOOLS_DIR) the JDK. The render layers' test
// reads the bake the data folder holds (the app's first start makes it) and
// is skipped without it: a test makes no bake, so two test files never make
// one at once.
const ready = fs.existsSync(path.join(minecraftLive.paperDir, PAPER[minecraftLive.minecraftVersion].jar))
  && fs.existsSync(path.join(minecraftLive.paperDir, 'versions')) && fs.existsSync(JAVA);

test('the block table holds Minecraft\'s light, faces and face shape pairs', { skip: !ready && 'needs Paper laid out in the data folder and the JDK in the tools folder', timeout: 300000 }, async () => {
  const states = opaqueStates(fs.readFileSync(await minecraftLive.blockTable(), 'utf8'));
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

// Minecraft's directions, by Direction.ordinal().
const [DOWN, UP, NORTH, SOUTH, WEST, EAST] = [0, 1, 2, 3, 4, 5];
const ALL = [DOWN, UP, NORTH, SOUTH, WEST, EAST];

test('the block table holds Minecraft\'s fluids, render shapes and skipRendering', { skip: !ready && 'needs Paper laid out in the data folder and the JDK in the tools folder', timeout: 300000 }, async () => {
  const states = opaqueStates(fs.readFileSync(await minecraftLive.blockTable(), 'utf8'));
  const id = (block, properties = {}) => states.idOf(block, properties);
  // The directions state `a` skips rendering toward neighbour `b` in.
  const skipped = (a, b) => ALL.filter((direction) => states.skips(a, b, direction));
  const glass = id('minecraft:glass');
  assert.deepEqual(skipped(glass, glass), ALL);
  assert.deepEqual(skipped(glass, id('minecraft:stone')), []);
  assert.deepEqual(skipped(id('minecraft:stone'), id('minecraft:stone')), [], 'stone hides faces by its full faces, not by skipRendering');
  assert.deepEqual(skipped(id('minecraft:red_stained_glass'), id('minecraft:red_stained_glass')), ALL);
  assert.deepEqual(skipped(id('minecraft:red_stained_glass'), id('minecraft:blue_stained_glass')), [], 'two colours of stained glass show the face between them');
  assert.deepEqual(skipped(glass, id('minecraft:red_stained_glass')), []);
  for (const block of ['minecraft:tinted_glass', 'minecraft:ice', 'minecraft:slime_block', 'minecraft:honey_block', 'minecraft:powder_snow']) {
    assert.deepEqual(skipped(id(block), id(block)), ALL, block);
  }
  assert.deepEqual(skipped(id('minecraft:ice'), id('minecraft:packed_ice')), []);
  assert.deepEqual(skipped(id('minecraft:oak_leaves', { persistent: 'true' }), id('minecraft:oak_leaves', { persistent: 'true' })), [], 'leaves show the faces between them');
  // Mangrove roots hide faces from each other along the vertical axis only.
  const roots = id('minecraft:mangrove_roots');
  assert.deepEqual(skipped(roots, roots), [DOWN, UP]);
  // A pane hides its face toward a pane it joins, and toward panes above and below.
  const pane = (sides) => id('minecraft:glass_pane', Object.fromEntries(['north', 'south', 'east', 'west'].map((side) => [side, String(sides.includes(side))])));
  assert.deepEqual(skipped(pane(['east']), pane(['west'])), [DOWN, UP, EAST]);
  assert.deepEqual(skipped(pane(['east']), pane([])), [DOWN, UP]);
  assert.deepEqual(skipped(pane([]), pane([])), [DOWN, UP]);
  assert.deepEqual(skipped(pane(['east']), id('minecraft:iron_bars', { west: 'true' })), [], 'a pane and bars are two blocks');
  // Water hides its faces toward every state that holds water, and nothing else.
  const water = id('minecraft:water', { level: '0' });
  const wetStair = id('minecraft:oak_stairs', { waterlogged: 'true' });
  assert.deepEqual(skipped(water, id('minecraft:water', { level: '5' })), ALL);
  assert.deepEqual(skipped(water, wetStair), ALL);
  assert.deepEqual(skipped(water, id('minecraft:kelp')), ALL);
  assert.deepEqual(skipped(water, id('minecraft:oak_stairs', { waterlogged: 'false' })), []);
  assert.deepEqual(skipped(water, id('minecraft:lava', { level: '0' })), []);
  assert.deepEqual(skipped(wetStair, water), [], 'a stair shows its faces toward water');
  // Fluids: the state of the fluid's block, air for none.
  assert.equal(states.fluid[wetStair], water);
  assert.equal(states.fluid[id('minecraft:oak_stairs', { waterlogged: 'false' })], 0);
  for (const block of ['minecraft:kelp', 'minecraft:kelp_plant', 'minecraft:seagrass', 'minecraft:bubble_column']) assert.equal(states.fluid[id(block)], water, block);
  assert.equal(states.fluid[id('minecraft:water', { level: '3' })], id('minecraft:water', { level: '3' }));
  assert.equal(states.fluid[id('minecraft:water', { level: '12' })], id('minecraft:water', { level: '8' }), 'falling water is one fluid state');
  assert.equal(states.fluid[id('minecraft:stone_slab', { type: 'bottom', waterlogged: 'true' })], water);
  // Render shapes: a liquid, air, light and a barrier are drawn as no model.
  for (const block of ['minecraft:water', 'minecraft:lava', 'minecraft:air', 'minecraft:light', 'minecraft:barrier']) assert.equal(states.draw[id(block)] & 1, 0, block);
  for (const block of ['minecraft:stone', 'minecraft:glass', 'minecraft:kelp', 'minecraft:chest']) assert.equal(states.draw[id(block)] & 1, 1, block);
});

// The block table and the render layers of the bake for it, as the data
// folder holds them.
const table = minecraftLive.blockTableFile();
const layers = fs.existsSync(table) && minecraftLive.bakedFile('render-layers.tsv');

test('the render layers are Minecraft\'s client\'s', { skip: !(layers && fs.existsSync(layers)) && 'needs the viewer\'s bake of the current sources in the data folder (the app\'s first start makes it)' }, async () => {
  const text = fs.readFileSync(layers, 'utf8');
  const states = new BlockStates(fs.readFileSync(table, 'utf8'), text);
  const lines = text.split('\n').filter(Boolean);
  const names = lines.filter((line) => line.startsWith('layer\t')).map((line) => line.split('\t')[1]);
  assert.deepEqual(lines.filter((line) => line.startsWith('layer\t')), ['layer\tSOLID\t0', 'layer\tCUTOUT_MIPPED\t0', 'layer\tCUTOUT\t0', 'layer\tTRANSLUCENT\t1', 'layer\tTRIPWIRE\t1']);
  const layer = (block, properties = {}) => {
    const [, own, fluid] = lines[names.length + states.idOf(block, properties)].split('\t');
    return [names[own], names[fluid]];
  };
  for (const block of ['minecraft:white_stained_glass', 'minecraft:black_stained_glass_pane', 'minecraft:tinted_glass', 'minecraft:ice', 'minecraft:frosted_ice', 'minecraft:slime_block', 'minecraft:honey_block', 'minecraft:nether_portal']) {
    assert.deepEqual(layer(block), ['TRANSLUCENT', 'SOLID'], block);
  }
  assert.deepEqual(layer('minecraft:glass'), ['CUTOUT', 'SOLID']);
  assert.deepEqual(layer('minecraft:glass_pane'), ['CUTOUT_MIPPED', 'SOLID']);
  assert.deepEqual(layer('minecraft:torch'), ['CUTOUT', 'SOLID']);
  assert.deepEqual(layer('minecraft:oak_leaves'), ['CUTOUT_MIPPED', 'SOLID'], 'leaves are cut out under Fancy graphics');
  assert.deepEqual(layer('minecraft:tripwire'), ['TRIPWIRE', 'SOLID']);
  assert.deepEqual(layer('minecraft:stone'), ['SOLID', 'SOLID']);
  assert.deepEqual(layer('minecraft:water'), ['SOLID', 'TRANSLUCENT'], 'water is drawn as its fluid, translucent');
  assert.deepEqual(layer('minecraft:lava'), ['SOLID', 'SOLID']);
  assert.deepEqual(layer('minecraft:kelp'), ['CUTOUT', 'TRANSLUCENT']);
  assert.deepEqual(layer('minecraft:oak_stairs', { waterlogged: 'true' }), ['SOLID', 'TRANSLUCENT']);
  // The page's draw bits: 2 the model blended, 4 the fluid blended.
  assert.equal(states.draw[states.idOf('minecraft:white_stained_glass', {})], 1 | 2);
  assert.equal(states.draw[states.idOf('minecraft:tripwire', {})], 1 | 2);
  assert.equal(states.draw[states.idOf('minecraft:oak_stairs', { waterlogged: 'true' })], 1 | 4);
  assert.equal(states.draw[states.idOf('minecraft:water', {})], 4);
  assert.equal(states.draw[states.idOf('minecraft:glass', {})], 1);
});
