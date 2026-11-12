'use strict';

const test = require('node:test');
const assert = require('node:assert');
let readPatch; let writePatch; let Changes; let InputBlocks; let readBlueprint; let inputOf;
test.before(async () => ({ readPatch, writePatch, Changes, InputBlocks, readBlueprint, inputOf } = await import('../web/live-patch.mjs')));

test('a patch reads back as written', () => {
  const patch = { entries: [{ id: 'minecraft:redstone_lamp', properties: [['lit', 'true']] }], bounds: [0, 64, 0, 4, 66, 2], columns: 1, changes: [[2, 65, 1, 7], [-3, -64, 9, 0]] };
  assert.deepEqual(readPatch(writePatch(patch)), patch);
  assert.throws(() => readPatch(Uint8Array.of(...writePatch(patch), 0)), /trailing bytes/);
});

test('the changes since a base merge into one patch, each position at its latest block', () => {
  const changes = new Changes();
  changes.add({ entries: [{ id: 'minecraft:a', properties: [] }], bounds: [0, 0, 0, 1, 1, 1], columns: 1, changes: [[0, 0, 0, 5], [1, 0, 0, 2]] });
  changes.add({ entries: [{ id: 'minecraft:b', properties: [] }], bounds: [0, 0, 0, 1, 1, 2], columns: 1, changes: [[0, 0, 0, 6]] });
  assert.deepEqual(readPatch(changes.patch()), {
    entries: [{ id: 'minecraft:a', properties: [] }, { id: 'minecraft:b', properties: [] }],
    bounds: [0, 0, 0, 1, 1, 2], columns: 1, changes: [[0, 0, 0, 6], [1, 0, 0, 2]],
  });
});

test('the page knows which blocks take a click, from the blueprint and every patch', () => {
  const blueprint = { palette: [{ id: 'minecraft:air', properties: [] }, { id: 'minecraft:stone', properties: [] }, { id: 'minecraft:lever', properties: [['powered', 'false']] }, { id: 'minecraft:oak_button', properties: [] }], sections: [] };
  const indices = new Uint16Array(4096);
  indices[(1 << 8) | (2 << 4) | 3] = 2; // x 3, y 1, z 2 of section 0, 0, 0
  indices[0] = 1;
  blueprint.sections.push({ x: -1, y: 4, z: 0, indices });
  const blocks = new InputBlocks();
  blocks.load(blueprint);
  assert.strictEqual(blocks.at(-16 + 3, 64 + 1, 2), 'toggle');
  assert.strictEqual(blocks.at(-16, 64, 0), null, 'stone takes no click');
  // A patch brings a button (a palette entry it adds), takes the lever away.
  blocks.apply({ entries: [{ id: 'minecraft:stone_button', properties: [] }], changes: [[5, 70, 5, 4], [-13, 65, 2, 0]] });
  assert.strictEqual(blocks.at(5, 70, 5), 'press');
  assert.strictEqual(blocks.at(-13, 65, 2), null);
  // A right click takes a door, trapdoor, fence gate, repeater, comparator, note block, daylight detector and decorated pot, of any wood or copper, but not iron.
  for (const id of ['oak_door', 'waxed_oxidized_copper_door', 'spruce_trapdoor', 'copper_trapdoor', 'crimson_fence_gate', 'repeater', 'comparator', 'note_block', 'daylight_detector', 'decorated_pot']) assert.strictEqual(inputOf(`minecraft:${id}`), 'use', id);
  for (const id of ['iron_door', 'iron_trapdoor', 'iron_bars', 'oak_fence', 'oak_slab', 'redstone_lamp', 'stone']) assert.strictEqual(inputOf(`minecraft:${id}`), null, id);
  // Redstone dust takes one as a dot only.
  const wire = (sides) => ['north', 'east', 'south', 'west'].map((side, i) => [side, sides[i]]).concat([['power', '0']]);
  assert.strictEqual(inputOf('minecraft:redstone_wire', wire(['none', 'none', 'none', 'none'])), 'use');
  assert.strictEqual(inputOf('minecraft:redstone_wire', wire(['side', 'side', 'side', 'side'])), null);
  assert.strictEqual(inputOf('minecraft:redstone_wire', wire(['side', 'none', 'up', 'none'])), null);
  const bytes = require('../server/blueprint').blocksBlueprint([[1, 64, 1, 'minecraft:lever[face=floor,facing=north,powered=true]']]);
  assert.strictEqual(readBlueprint(new Uint8Array(bytes)).sections[0].indices.filter(Boolean).length, 1);
});
