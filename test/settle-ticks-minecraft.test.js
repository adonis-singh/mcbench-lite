'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { tablesFolder } = require('../server/minecraft-tables');
const { DEFAULT_SETTLE_TICKS } = require('../server/minecraft-dimension');
const { MC_VERSION } = require('../server/assets');

const TABLE = path.join(tablesFolder(MC_VERSION), 'blocks.txt');

// The default settle against Minecraft's fluids (the engine's table: each
// flowing fluid's tick delay and drop-off in the overworld, and the amount of
// each fluid state): the steps of the longest spread across flat ground, from
// a source's amount down to 1, at the slowest tick delay.
test('the default settle is the longest fluid spread on flat ground at the slowest fluid\'s tick delay', { skip: !fs.existsSync(TABLE) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)' }, () => {
  const lines = fs.readFileSync(TABLE, 'utf8').split('\n').map((line) => line.split('\t'));
  const rules = lines.filter(([type]) => type === 'fluidrule').map(([, key, delay, dropOff]) => ({ key, delay: Number(delay), dropOff: Number(dropOff) }));
  const source = Math.max(...lines.filter(([type, , , , , , source]) => type === 'fluid' && source === '1').map(([, , , , , amount]) => Number(amount)));
  assert.deepStrictEqual(rules.map(({ key }) => key).sort(), ['minecraft:flowing_lava', 'minecraft:flowing_water', 'minecraft:lava', 'minecraft:water']);
  const steps = Math.max(...rules.map(({ dropOff }) => Math.floor((source - 1) / dropOff)));
  const slowest = Math.max(...rules.map(({ delay }) => delay));
  assert.deepStrictEqual([source, steps, slowest], [8, 7, 30]);
  assert.strictEqual(DEFAULT_SETTLE_TICKS, steps * slowest);
});
