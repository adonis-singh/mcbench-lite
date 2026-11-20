'use strict';

// node normalize-cases.js > tests/golden/normalize.jsonl
//
// mc-dsl.js createBlockNormalizer's answers, for src/normalize.rs's
// differential test: a test registry (golden-registry.json plus a block
// with an index-like state and one with no states) and specs covering every
// branch: strings with and without states, malformed states, objects with
// and without states, non-string ids, prefixes and spaces, unknown names
// with and without close matches, Object.prototype names as blocks and as
// states, index-like keys, and state values of every JSON type.

const path = require('path');
const { createBlockNormalizer } = require(path.resolve(__dirname, '../../../../server/mc-dsl.js'));

const registry = {
  ...require('./golden-registry.json'),
  note_block: [{ note: ['0', '1', '24'], instrument: ['harp', 'bass'] }, { note: '0', instrument: 'harp' }],
  '7': [{ '3': ['a', 'b'], x: ['1'] }, { '3': 'a', x: '1' }],
};
const normalize = createBlockNormalizer(registry);
const specs = [
  'stone', 'minecraft:stone', ' stone ', 'minecraft: stone', 'STONE', '', 'minecraft:', 'air', 'minecraft:air', 'air[waterlogged=true]',
  'oak_stairs[facing=east]', 'oak_stairs[facing=east,half=top]', 'oak_stairs[ facing = east ]', 'oak_stairs[facing=up]', 'oak_stairs[facing]',
  'oak_stairs[facing=east', 'oak_stairs[,facing=east,]', 'oak_stairs[facing=east,facing=west]', 'oak_stairs[shape=straight]', 'stone[a=b]',
  'water[level=3]', 'water[level=03]', 'ston', 'stairs', 'oak', 'o', 'xyz', 'constructor', 'toString', '__proto__', 'hasOwnProperty', 'valueOf',
  'oak_stairs[constructor=x]', 'oak_stairs[toString=x]', '7', '7[3=a]', '7[3=c]', '7[x=1,3=b]', 'note_block[note=24]', 'note_block[note=25]',
  'é', 'stone[]', 'stone[=x]', 'gold_block[x=y=z]',
  { id: 'stone' }, { id: 'minecraft:stone', states: {} }, { id: 'stone', states: null }, { id: 'stone', states: [] }, { id: 'stone', states: 'x' },
  { id: 5 }, { id: null }, {}, { states: {} }, 5, null, true, ['stone'],
  { id: 'oak_stairs', states: { facing: 'east' } }, { id: 'oak_stairs', states: { half: 'top', facing: 'east' } },
  { id: 'oak_stairs', states: { facing: ['east'] } }, { id: 'oak_stairs', states: { facing: [['east']] } }, { id: 'oak_stairs', states: { facing: 1 } },
  { id: 'oak_stairs', states: { facing: true } }, { id: 'oak_stairs', states: { facing: null } }, { id: 'oak_stairs', states: { facing: {} } },
  { id: 'oak_stairs', states: { facing: [null, 'x'] } }, { id: 'water', states: { level: 3 } }, { id: 'water', states: { level: 3.0 } },
  { id: 'water', states: { level: 1e21 } }, { id: 'water', states: { level: -0 } }, { id: 'water', states: { level: 0.5 } },
  { id: 'note_block', states: { note: 24, instrument: 'bass' } }, { id: 'note_block', states: { 1: 'x', note: '0' } },
  { id: '7', states: { x: '1', 3: 'b' } }, { id: '7', states: { 3: 'c' } }, { id: 'stone', states: { constructor: 'x' } },
  { id: 'oak_stairs', states: JSON.parse('{"__proto__":"x"}') }, { id: 'oak_stairs', states: { valueOf: 'x' } },
  { id: 'constructor' }, { id: '__proto__' }, { id: 'isPrototypeOf' }, { id: ' minecraft:stone ' }, { id: 'minecraft:  stone' }, { id: '' },
  { id: 'gold_block', states: { a: 1 } }, { id: 'oak_stairs', states: { facing: 'east', bogus: 'x' } }, { id: 'oak_stairs', states: { bogus: 'x', facing: 'up' } },
];
for (const spec of specs) {
  let out;
  try { out = { ok: normalize(spec, 'palette entry') }; } catch (e) { out = { error: e.message }; }
  process.stdout.write(`${JSON.stringify({ spec, ...out })}\n`);
}
process.stdout.write(`${JSON.stringify({ registry })}\n`);
