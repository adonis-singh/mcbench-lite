'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { BlockTints } = require('../server/block-tints');
const { testBlockStates } = require('./helpers/block-table');
const { tintsText } = require('./helpers/block-tints');

// The tints of states written for the test, read by state id.
const STATES = testBlockStates([
  { name: 'minecraft:grass_block[snowy=false]' },
  { name: 'minecraft:grass_block[snowy=true]' },
  { name: 'minecraft:stone' },
]);

test('the tints are read by state id: the world colour of each tint index, and the particles\'', () => {
  const tints = new BlockTints(tintsText(STATES, {
    'minecraft:grass_block[snowy=false]': { world: [0x91bd59, 0x91bd59], single: 0x7cbd6b, particle: 0xffffff, faces: '0' },
    'minecraft:grass_block[snowy=true]': { world: [0x91bd59, 0x123456], particle: 0x91bd59 },
  }), STATES);
  assert.equal(tints.indices, 2);
  const grass = STATES.idOf('minecraft:grass_block', { snowy: 'true' });
  assert.deepEqual([...tints.world.subarray(grass * 2, grass * 2 + 2)], [0x91bd59, 0x123456]);
  assert.equal(tints.particle[grass], 0x91bd59);
  assert.equal(tints.particle[STATES.idOf('minecraft:grass_block', { snowy: 'false' })], 0xffffff);
  const stone = STATES.idOf('minecraft:stone', {});
  assert.deepEqual([...tints.world.subarray(stone * 2, stone * 2 + 2)], [0xffffff, 0xffffff]);

});

test('tints of other states than the block table\'s are refused', () => {
  const lines = tintsText(STATES).split('\n');
  assert.throws(() => new BlockTints(lines.slice(0, -2).join('\n'), STATES), /The tints have 3 states, the block table 4/);
  [lines[2], lines[3]] = [lines[3], lines[2]];
  assert.throws(() => new BlockTints(lines.join('\n'), STATES), /The tints name state 1 minecraft:grass_block\[snowy=true\]/);
});
