'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { MinecraftFacts } = require('../server/minecraft-facts');
const { tablesFolder } = require('../server/minecraft-tables');
const { PaletteStore } = require('../server/world');
const { MC_VERSION } = require('../server/assets');

const TABLE = path.join(tablesFolder(MC_VERSION), 'blocks.txt');

// The lines of the engine's table MinecraftFacts reads, for a test: blocks
// with their class, properties and states (the default second), an item tag,
// what each state placed alone does and where a full block holds it.
const TEXT = [
  'block\tminecraft:stone\tnet.minecraft.world.level.block.Block\t-\t0\t1\t0\t0.6\t1.0\t1.0\t-\t-\t6.0',
  'state\t0\t-',
  'block\ttest:torch\tnet.minecraft.world.level.block.TorchBlock\t-\t1\t2\t2\t0.6\t1.0\t1.0\t-\tBaseTorchBlock\t0.0',
  'property\tlit\ttrue,false',
  'state\t1\t0',
  'state\t2\t1',
  'itemtag\tminecraft:lectern_books\tminecraft:written_book,minecraft:writable_book',
  'alone\t0,2,0',
  'held\t1:1,2:0',
].join('\n');

test('the facts name states by their properties, defaults where none is named, and read what each does alone', () => {
  const facts = new MinecraftFacts(TEXT);
  assert.deepStrictEqual([facts.stateId('minecraft:stone'), facts.stateId('test:torch'), facts.stateId('test:torch', { lit: 'true' })], [0, 2, 1]);
  assert.throws(() => facts.stateId('test:torch', { lit: 'maybe' }), /no block state test:torch\[lit=maybe\]/);
  assert.throws(() => facts.stateId('test:lamp'), /no block test:lamp/);
  assert.strictEqual(facts.className('test:torch'), 'TorchBlock');
  assert.deepStrictEqual([facts.is('test:torch', 'TorchBlock'), facts.is('test:torch', 'BaseTorchBlock'), facts.is('test:torch', 'Block')], [true, true, false]);
  assert.deepStrictEqual(facts.defaults('test:torch'), { lit: 'false' });
  assert.deepStrictEqual([...facts.itemTag('minecraft:lectern_books')], ['minecraft:written_book', 'minecraft:writable_book']);
  assert.deepStrictEqual([...facts.survives], [1, 0, 0]);
  assert.deepStrictEqual(facts.heldBy('test:torch', { lit: 'true' }), [[0, -1, 0]]);
  assert.deepStrictEqual(facts.heldBy('test:torch', {}), []);
  const palette = new PaletteStore();
  palette.intern('minecraft:stone');
  palette.intern('test:torch', { lit: 'true' });
  assert.deepStrictEqual([...facts.lastPass(palette)], [0, 0, 1]);
  palette.intern('test:torch');
  assert.deepStrictEqual([...facts.lastPass(palette)], [0, 0, 1, 0], 'an entry added later is read then');
});

test('Minecraft\'s table: a lectern takes written and writable books, beds are BedBlock, and three blocks are air', { skip: !fs.existsSync(TABLE) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)' }, () => {
  const facts = new MinecraftFacts(fs.readFileSync(TABLE, 'utf8'));
  assert.deepStrictEqual([...facts.itemTag('minecraft:lectern_books')].sort(), ['minecraft:writable_book', 'minecraft:written_book']);
  const beds = [...facts.blocks.keys()].filter((id) => facts.className(id) === 'BedBlock');
  assert.strictEqual(beds.length, 16);
  assert.ok(beds.every((id) => id.endsWith('_bed')));
  assert.deepStrictEqual([...facts.blocks.keys()].filter((id) => facts.isAir(id)), ['minecraft:air', 'minecraft:void_air', 'minecraft:cave_air']);
});
