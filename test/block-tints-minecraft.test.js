'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');
const { test } = require('node:test');

const { minecraftLive } = require('../server/minecraft-live');
const { MC_VERSION } = require('../server/assets');
const { BIOME } = require('../server/minecraft-dimension');
const { BlockStates } = require('../server/block-states');
const { BlockTints } = require('../server/block-tints');
const { readZipEntry } = require('../server/zip-entry');
const { blockIcon, _internal: { decodePng, icon, loadAssets } } = require('../server/block-icon');

// The colours Minecraft's client tints block states with in the app's world,
// in the tints file the viewer's bake writes (McbenchClient.java), each
// checked against what Minecraft says a second way where it says it: the
// client jar's colour maps decoded here (not by ImageIO, which the program
// decodes them with in NativeImage's place) and looked up by its biome's
// climate, its item definitions' tints, its biome's water colour, its own
// formulas for dust and stems. It reads the bake the data folder
// (MCBENCH_DATA_DIR) holds for its block table, and the client jar beside it
// (the app's first start makes both), and is skipped without them: a test
// makes no bake, so two test files never make one at once.
const client = path.join(minecraftLive.blueMap.root, 'data', `minecraft-client-${MC_VERSION}.jar`);
const TABLE = minecraftLive.blockTableFile();
const TINTS = fs.existsSync(TABLE) && minecraftLive.bakedFile('tints.tsv');
const skip = !(TINTS && fs.existsSync(TINTS) && fs.existsSync(client)) && 'needs the viewer\'s bake of the current sources in the data folder (the app\'s first start makes it)';

const jarJson = (name) => JSON.parse(readZipEntry(client, name).toString('utf8'));

// An 8-bit RGB or RGBA PNG's pixels as 0xRRGGBB, row by row.
function pixels(png) {
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  const channels = { 2: 3, 6: 4 }[png[25]];
  assert.ok(png[24] === 8 && channels && png[28] === 0, 'an 8-bit RGB or RGBA PNG, not interlaced');
  const parts = [];
  for (let at = 8; at < png.length;) {
    const length = png.readUInt32BE(at);
    if (png.toString('latin1', at + 4, at + 8) === 'IDAT') parts.push(png.subarray(at + 8, at + 8 + length));
    at += length + 12;
  }
  const raw = zlib.inflateSync(Buffer.concat(parts));
  const stride = width * channels;
  const rows = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x += 1) {
      const left = x >= channels ? rows[y * stride + x - channels] : 0;
      const up = y > 0 ? rows[(y - 1) * stride + x] : 0;
      const corner = x >= channels && y > 0 ? rows[(y - 1) * stride + x - channels] : 0;
      const estimate = left + up - corner;
      const predictor = [0, left, up, (left + up) >> 1,
        Math.abs(estimate - left) <= Math.abs(estimate - up) && Math.abs(estimate - left) <= Math.abs(estimate - corner) ? left
          : Math.abs(estimate - up) <= Math.abs(estimate - corner) ? up : corner][filter];
      rows[y * stride + x] = (raw[y * (stride + 1) + 1 + x] + predictor) & 255;
    }
  }
  return Array.from({ length: width * height }, (_, i) => rows.readUIntBE(i * channels, 3));
}

// Minecraft's colour map lookup (ColorMapColorUtil.get), its climate's
// floats widened to doubles as Biome does.
function colormap(name, temperature, downfall) {
  const map = pixels(readZipEntry(client, `assets/minecraft/textures/colormap/${name}.png`));
  const t = Math.min(Math.max(Math.fround(temperature), 0), 1);
  const d = Math.min(Math.max(Math.fround(downfall), 0), 1) * t;
  return map[(Math.trunc((1 - d) * 255) << 8) | Math.trunc((1 - t) * 255)];
}

// RedStoneWireBlock's colour of a power, in its floats (ARGB.colorFromFloat).
function dust(power) {
  const f = Math.fround(power / 15);
  const clamp = (value) => Math.min(Math.max(value, 0), 1);
  const channels = [Math.fround(Math.fround(f * 0.6) + (f > 0 ? 0.4 : 0.3)), clamp(Math.fround(Math.fround(f * f * 0.7) - 0.5)), clamp(Math.fround(Math.fround(f * f * 0.6) - 0.7))];
  return channels.reduce((color, channel) => (color << 8) | Math.floor(Math.fround(channel * 255)), 0);
}

// An item definition's first tint source's constant, as 0xRRGGBB.
const itemConstant = (item) => jarJson(`assets/minecraft/items/${item}.json`).model.tints[0].value & 0xffffff;

test('the tints are Minecraft\'s client\'s, in the world\'s biome', { skip }, async () => {
  const tintsFile = TINTS;
  const states = new BlockStates(fs.readFileSync(TABLE, 'utf8'), fs.readFileSync(minecraftLive.bakedFile('render-layers.tsv'), 'utf8'));
  const tints = new BlockTints(fs.readFileSync(tintsFile, 'utf8'), states);
  const lines = fs.readFileSync(tintsFile, 'utf8').split('\n').filter(Boolean).slice(1).map((line) => line.split('\t'));
  const of = (block, properties = {}) => {
    const id = states.idOf(block, properties);
    const [, world, single, particle, faces] = lines[id];
    return { world: world.split(',').map((color) => parseInt(color, 16)), single: parseInt(single, 16), particle: parseInt(particle, 16), faces, id };
  };
  assert.equal(tints.indices, 2, 'block models use tint indices 0 and 1');

  const biome = jarJson(`data/minecraft/worldgen/biome/${BIOME.replace('minecraft:', '')}.json`);
  assert.equal(biome.effects.grass_color ?? biome.effects.foliage_color ?? biome.effects.grass_color_modifier, undefined, 'the biome has no grass or foliage override or modifier');
  const grass = colormap('grass', biome.temperature, biome.downfall);
  const foliage = colormap('foliage', biome.temperature, biome.downfall);
  const dryFoliage = colormap('dry_foliage', biome.temperature, biome.downfall);
  // The colour with no level: the grass item's tint source, grass at 0.5, 1.
  const grassItem = jarJson('assets/minecraft/items/grass_block.json').model.tints[0];
  const defaultGrass = colormap('grass', grassItem.temperature, grassItem.downfall);
  assert.deepEqual([grass, foliage, defaultGrass], [0x91bd59, 0x77ab2f, 0x7cbd6b], 'plains\' grass and foliage, and the default grass, as the wiki gives them');

  // Grass blocks: top and side overlay tinted alike (index 0), their
  // particles their dirt, untinted.
  const model = jarJson('assets/minecraft/models/block/grass_block.json');
  const tinted = model.elements.flatMap((element) => Object.entries(element.faces).filter(([, face]) => face.tintindex === 0).map(([side]) => side));
  assert.deepEqual(tinted.sort(), ['east', 'north', 'south', 'up', 'west'], 'its top and the overlay of each side are of tint index 0');
  for (const snowy of ['false', 'true']) {
    assert.deepEqual(of('minecraft:grass_block', { snowy }), { world: [grass, grass], single: defaultGrass, particle: 0xffffff, faces: '0', id: states.idOf('minecraft:grass_block', { snowy }) });
  }
  for (const block of ['minecraft:short_grass', 'minecraft:fern', 'minecraft:bush', 'minecraft:potted_fern']) {
    assert.deepEqual([of(block).world, of(block).single, of(block).particle], [[grass, grass], defaultGrass, grass], block);
  }
  for (const half of ['lower', 'upper']) {
    for (const block of ['minecraft:tall_grass', 'minecraft:large_fern']) assert.deepEqual(of(block, { half }).world, [grass, grass], `${block} ${half}`);
  }
  // Sugar cane: grass in the world, untinted with no level.
  assert.deepEqual([of('minecraft:sugar_cane').world, of('minecraft:sugar_cane').single], [[grass, grass], 0xffffff]);
  // Petals: the stems (index 1) grass, the flowers (index 0) untinted.
  assert.deepEqual([of('minecraft:pink_petals').world, of('minecraft:pink_petals').faces], [[0xffffff, grass], '1']);

  // Leaves: the biome's foliage in the world, the default (the oak leaves
  // item's constant) with no level; spruce and birch their own constants.
  for (const block of ['minecraft:oak_leaves', 'minecraft:jungle_leaves', 'minecraft:acacia_leaves', 'minecraft:dark_oak_leaves', 'minecraft:mangrove_leaves', 'minecraft:vine']) {
    assert.deepEqual([of(block).world, of(block).single, of(block).particle], [[foliage, foliage], itemConstant('oak_leaves'), foliage], block);
  }
  assert.deepEqual([of('minecraft:spruce_leaves').world, of('minecraft:spruce_leaves').single], [[itemConstant('spruce_leaves'), itemConstant('spruce_leaves')], itemConstant('spruce_leaves')]);
  assert.deepEqual([of('minecraft:birch_leaves').world, of('minecraft:birch_leaves').single], [[itemConstant('birch_leaves'), itemConstant('birch_leaves')], itemConstant('birch_leaves')]);
  // Cherry and pale oak leaves have tinted faces Minecraft leaves white.
  for (const block of ['minecraft:cherry_leaves', 'minecraft:pale_oak_leaves', 'minecraft:bamboo']) {
    assert.deepEqual([of(block).world, of(block).single, of(block).faces], [[0xffffff, 0xffffff], 0xffffff, '0'], block);
  }
  assert.deepEqual(of('minecraft:leaf_litter').world, [dryFoliage, dryFoliage], 'leaf litter takes the dry foliage map');

  // Lily pads: their colour in a level; with none, the lily pad item's.
  assert.deepEqual([of('minecraft:lily_pad').world, of('minecraft:lily_pad').single], [[0x208030, 0x208030], itemConstant('lily_pad')]);

  // Water, its cauldron and bubble columns: the biome's water colour; with
  // no level, none.
  for (const [block, properties] of [['minecraft:water', { level: '0' }], ['minecraft:water', { level: '7' }], ['minecraft:water_cauldron', { level: '3' }], ['minecraft:bubble_column', { drag: 'true' }]]) {
    assert.deepEqual([of(block, properties).world, of(block, properties).single], [[biome.effects.water_color, biome.effects.water_color], 0xffffff], block);
  }

  // Redstone dust by power, by RedStoneWireBlock's own formula.
  for (const power of [0, 7, 15]) {
    const { world, single } = of('minecraft:redstone_wire', { power: String(power) });
    assert.deepEqual([world, single], [[dust(power), dust(power)], dust(power)], `dust of power ${power}`);
  }
  assert.deepEqual([0, 7, 15].map(dust), [0x4c0000, 0xad0000, 0xff3200]);

  // Stems by age (ARGB.color(age * 32, 255 - age * 8, age * 4)); attached
  // ones the colour of the oldest.
  for (const block of ['minecraft:melon_stem', 'minecraft:pumpkin_stem']) {
    for (let age = 0; age <= 7; age += 1) assert.deepEqual(of(block, { age: String(age) }).world, Array(2).fill((age * 32 << 16) | ((255 - age * 8) << 8) | (age * 4)), `${block} ${age}`);
  }
  for (const block of ['minecraft:attached_melon_stem', 'minecraft:attached_pumpkin_stem']) {
    assert.deepEqual(of(block, { facing: 'north' }).world, of('minecraft:melon_stem', { age: '7' }).world, block);
  }

  // Untinted blocks.
  for (const block of ['minecraft:stone', 'minecraft:glass', 'minecraft:oak_planks', 'minecraft:dirt', 'minecraft:lava', 'minecraft:cauldron', 'minecraft:torch']) {
    assert.deepEqual([of(block).world, of(block).single, of(block).particle, of(block).faces], [[0xffffff, 0xffffff], 0xffffff, 0xffffff, '-'], block);
  }
});

// The baked pack's colour of every state (McbenchBake.java's format), by
// state name.
function packTints(pack) {
  let at = 8;
  const u8 = () => pack[at++];
  const u16 = () => { at += 2; return pack.readUInt16LE(at - 2); };
  const u32 = () => { at += 4; return pack.readUInt32LE(at - 4); };
  const str = () => { const length = u16(); at += length; return pack.toString('utf8', at - length, at); };
  const blocks = [];
  for (let count = u32(), i = 0; i < count; i += 1) {
    const id = str();
    const first = u32();
    const properties = [];
    for (let n = u8(), p = 0; p < n; p += 1) {
      const name = str();
      u8();
      const values = [];
      for (let v = u8(), k = 0; k < v; k += 1) values.push(str());
      properties.push({ name, values });
    }
    blocks.push({ id, first, properties });
  }
  const stateCount = u32();
  const states = Array.from({ length: stateCount }, (_, i) => pack.readUInt32LE(at + i * 18 + 10));
  at += stateCount * 18;
  const faceSets = u32();
  at += faceSets * 6;
  const shapes = u16();
  at += Math.ceil((shapes * shapes) / 8);
  const cullClasses = u32();
  at += cullClasses * 12;
  const cullShapes = u16();
  at += Math.ceil((cullShapes * cullShapes) / 8);
  for (let rows = u16(), r = 0; r < rows; r += 1) {
    u16();
    const masks = u16();
    at += masks;
  }
  at += 13;
  const colors = Array.from({ length: u32() }, (_, i) => [0, 4, 8].reduce((color, offset) => (color << 8) | Math.round(pack.readFloatLE(at + i * 12 + offset) * 255), 0));
  const byName = new Map();
  for (const { id, first, properties } of blocks) {
    const count = properties.reduce((product, { values }) => product * values.length, 1);
    for (let index = 0; index < count; index += 1) {
      const values = {};
      for (let p = properties.length - 1, rest = index; p >= 0; p -= 1) {
        values[properties[p].name] = properties[p].values[rest % properties[p].values.length];
        rest = Math.floor(rest / properties[p].values.length);
      }
      const keys = Object.keys(values).sort();
      byName.set(keys.length ? `${id}[${keys.map((key) => `${key}=${values[key]}`).join(',')}]` : id, colors[states[first + index]]);
    }
  }
  return byName;
}

test('the live viewer\'s pack tints every state Minecraft\'s client tints in the client\'s colour', { skip }, async () => {
  const lines = fs.readFileSync(TINTS, 'utf8').split('\n').filter(Boolean).slice(1).map((line) => line.split('\t'));
  const pack = packTints(fs.readFileSync(minecraftLive.bakedFile('web/mcbench-pack.bin')));
  let compared = 0;
  for (const [name, world, , , faces] of lines) {
    const colors = world.split(',');
    if (faces === '-' && colors.every((color) => color === 'ffffff')) continue;
    const color = parseInt(colors[faces === '-' ? 0 : Number(faces.split(',')[0])], 16);
    assert.equal(pack.get(name).toString(16), color.toString(16), name);
    compared += 1;
  }
  assert.ok(compared > 1700, `${compared} tinted states`);
  assert.equal(pack.get('minecraft:redstone_wire[east=none,north=none,power=7,south=none,west=none]'), dust(7));
});

// Through the app's own path, which finds the bake there and makes none.
test('a block\'s icon is multiplied by its breaking particles\' colour, and a name of no block has none', { skip }, async () => {
  const loaded = await loadAssets();
  const tints = (await require('../server/block-tints').blockTints());
  const states = await require('../server/block-states').blockStates();
  for (const name of ['oak_leaves', 'grass_block', 'redstone_wire', 'stone']) {
    const color = tints.particle[states.idOf(`minecraft:${name}`, {})];
    assert.deepEqual(decodePng(await blockIcon(name)), decodePng(icon(name, loaded, color)), name);
  }
  assert.equal(tints.particle[states.idOf('minecraft:grass_block', {})], 0xffffff, 'a grass block breaks into dirt, untinted');
  assert.equal(await blockIcon('glow_item_frame'), null, 'item frames have block definitions and are no blocks');
  assert.equal(await blockIcon('not_a_block'), null);
});
