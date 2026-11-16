'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { engineModule } = require('../scripts/wasm-node-engine');
const { tablesFolder } = require('../server/minecraft-tables');
const { MC_VERSION } = require('../server/assets');

// Which input each block takes, as the engine names it (CoreWorld.inputs,
// core/src/blocks/mod.rs input_of) for the palettes of its blueprints and
// patches, and as its input and click requests take it.
let wasm;
let tables;
let CoreWorld;
let readBlueprint;
let readPatch;
let InputBlocks;
test.before(async () => {
  ({ CoreWorld } = await import('../web/core-world.mjs'));
  ({ readBlueprint, readPatch, InputBlocks } = await import('../web/live-patch.mjs'));
  ({ module: wasm, tables } = await engineModule());
});

const text = ({ id, properties }) => `${id}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]`;

// Blocks on a floor of stone, each at x = 2i, y 65, z 1, as setblock lines.
function row(blocks) {
  return [`fill -1 64 0 ${2 * blocks.length} 64 2 minecraft:stone\n`, ...blocks.map((block, i) => `setblock ${2 * i} 65 1 minecraft:${block}\n`)].join('');
}

// The input the engine names for the block at each position of a blueprint.
function inputsAt(blueprint, inputs) {
  const index = new InputBlocks();
  index.load(blueprint, inputs);
  return index;
}

const NAMED = [
  ['lever[face=floor,facing=north,powered=false]', 'toggle'],
  ['stone_button[face=floor,facing=north,powered=false]', 'press'],
  ['oak_button[face=floor,facing=north,powered=false]', 'press'],
  ['polished_blackstone_button[face=floor,facing=north,powered=false]', 'press'],
  ['oak_door[facing=south,half=lower]', 'use'],
  ['copper_door[facing=south,half=lower]', 'use'],
  ['waxed_weathered_copper_door[facing=south,half=lower]', 'use'],
  ['iron_door[facing=south,half=lower]', null],
  ['spruce_trapdoor[facing=south]', 'use'],
  ['exposed_copper_trapdoor[facing=south]', 'use'],
  ['iron_trapdoor[facing=south]', null],
  ['oak_fence_gate[facing=north]', 'use'],
  ['repeater[facing=north]', 'use'],
  ['comparator[facing=north]', 'use'],
  ['note_block', 'use'],
  ['daylight_detector', 'use'],
  ['decorated_pot', 'use'],
  ['redstone_wire[north=none,east=none,south=none,west=none]', 'use'],
  ['redstone_wire[north=side,east=side,south=side,west=side]', 'use'],
  ['redstone_wire[north=side,east=none,south=side,west=none]', null],
  ['stone', null],
  ['chest[facing=north]', null],
  ['hopper', null],
  ['piston[facing=up]', null],
  ['redstone_lamp', null],
];

test('the engine names the input of each palette entry of a blueprint: a lever toggles, a button presses, the usable blocks but iron take a use', () => {
  const world = new CoreWorld(wasm, tables);
  const bytes = world.place(row(NAMED.map(([block]) => block)));
  const blueprint = readBlueprint(bytes);
  const inputs = world.inputs();
  assert.strictEqual(inputs.length, blueprint.palette.length, 'one input for each palette entry');
  const index = inputsAt(blueprint, inputs);
  NAMED.forEach(([block, input], i) => assert.strictEqual(index.at(2 * i, 65, 1), input, block));
  assert.strictEqual(index.at(0, 64, 0), null, 'the floor');
});

test('a patch carries the inputs of the palette entries it adds, and a click takes what the block takes', () => {
  const world = new CoreWorld(wasm, tables);
  const bytes = world.place(row(['lever[face=floor,facing=north,powered=false]', 'redstone_lamp', 'oak_door[facing=south,half=lower]', 'iron_door[facing=south,half=lower]', 'stone_button[face=floor,facing=north,powered=false]']));
  const blueprint = readBlueprint(bytes);
  const index = inputsAt(blueprint, world.inputs());
  assert.deepStrictEqual(world.tick(1).inputs, [], 'a patch that adds nothing');
  assert.strictEqual(world.click(0, 65, 1, 0), 'toggle');
  assert.strictEqual(world.click(4, 65, 1, 0), 'use');
  assert.strictEqual(world.click(6, 65, 1, 0), null, 'iron is not opened by hand');
  assert.strictEqual(world.click(8, 65, 1, 0), 'press');
  assert.strictEqual(world.click(8, 65, 1, 0), 'press', 'a pressed button takes the press, and stays pressed');
  assert.strictEqual(world.click(2, 65, 1, 0), null, 'the lamp takes nothing');
  const ticked = world.tick(1);
  const patch = readPatch(ticked.bytes);
  assert.strictEqual(ticked.inputs.length, patch.entries.length);
  const added = Object.fromEntries(patch.entries.map((entry, i) => [text(entry), ticked.inputs[i]]));
  assert.strictEqual(added['minecraft:lever[face=floor,facing=north,powered=true]'], 'toggle');
  assert.strictEqual(added['minecraft:oak_door[facing=south,half=lower,hinge=left,open=true,powered=false]'], 'use');
  assert.strictEqual(added['minecraft:stone_button[face=floor,facing=north,powered=true]'], 'press');
  index.apply(patch, ticked.inputs);
  assert.strictEqual(index.at(0, 65, 1), 'toggle', 'the pulled lever');
  assert.strictEqual(index.at(4, 65, 1), 'use', 'the open door');
  assert.strictEqual(index.at(6, 65, 1), null, 'the iron door');
  assert.strictEqual(index.at(8, 65, 1), 'press', 'the pressed button');
});

test('a click on dust: a dot becomes a cross and a lone cross a dot; a cross all four of whose sides connect passes, and nothing changes', () => {
  const world = new CoreWorld(wasm, tables);
  world.place('fill -3 64 -3 3 64 3 minecraft:stone\nsetblock 0 65 0 minecraft:redstone_wire[north=none,east=none,south=none,west=none]\n');
  world.tick(1);
  assert.strictEqual(world.click(0, 65, 0, 0), 'use');
  const toCross = readPatch(world.tick(1).bytes);
  assert.match(text(toCross.entries[0]), /north=side.*/);
  assert.strictEqual(world.click(0, 65, 0, 0), 'use');
  assert.strictEqual(readPatch(world.tick(1).bytes).changes.length, 1, 'back to a dot');
  // Dust on all four sides: the junction is a cross either way.
  const junction = readBlueprint(world.place('setblock 1 65 0 minecraft:redstone_wire\nsetblock -1 65 0 minecraft:redstone_wire\nsetblock 0 65 1 minecraft:redstone_wire\nsetblock 0 65 -1 minecraft:redstone_wire\n', { fresh: false }));
  const inputs = world.inputs();
  const at = (x, y, z) => {
    const section = junction.sections.find((s) => s.x === x >> 4 && s.y === y >> 4 && s.z === z >> 4);
    return section.indices[((y & 15) << 8) | ((z & 15) << 4) | (x & 15)];
  };
  assert.match(text(junction.palette[at(0, 65, 0)]), /east=side,north=side,power=0,south=side,west=side/);
  assert.strictEqual(inputs[at(0, 65, 0)], 'use', 'its state names a use');
  assert.strictEqual(world.click(0, 65, 0, 0), null, 'which passes');
  assert.throws(() => world.input('use', 0, 65, 0, 0), /IllegalArgumentException: No use input at 0, 65, 0/);
  assert.strictEqual(readPatch(world.tick(1).bytes).changes.length, 0, 'nothing changed');
});

// Every block of Minecraft's block table, in its default state, alone on the
// floor: the input the engine's palette names for it is the one kind of
// input it takes, and a click takes that one. A block whose placement the
// port does not model (it refuses: no click could reach it) is left out.
test('the engine takes exactly the input it names, for every block of the table', () => {
  const table = path.join(tablesFolder(MC_VERSION), 'blocks.txt');
  const all = fs.readFileSync(table, 'utf8').split('\n').filter((line) => line.startsWith('block\t')).map((line) => line.split('\t')[1]);
  assert.ok(all.length > 1000);
  const world = new CoreWorld(wasm, tables);
  const refused = new Set();
  let keys;
  let side;
  let blueprint;
  for (;;) {
    keys = all.filter((key) => !refused.has(key));
    side = Math.ceil(Math.sqrt(keys.length));
    const lines = [`fill 0 64 0 ${3 * side} 64 ${3 * side} minecraft:stone\n`];
    keys.forEach((key, i) => lines.push(`setblock ${3 * (i % side)} 65 ${3 * Math.floor(i / side)} ${key}\n`));
    try {
      blueprint = readBlueprint(world.place(lines.join('')));
      break;
    } catch (error) {
      const block = /^The engine failed: unported: .*\((minecraft:[a-z0-9_]+)/.exec(error.message)?.[1];
      if (!block) throw error;
      refused.add(block);
    }
  }
  assert.ok(refused.size < 50, `the port refuses to place ${[...refused].join(', ')}`);
  const index = inputsAt(blueprint, world.inputs());
  const named = { toggle: 0, press: 0, use: 0 };
  keys.forEach((key, i) => {
    const [x, z] = [3 * (i % side), 3 * Math.floor(i / side)];
    const input = index.at(x, 65, z);
    for (const kind of ['toggle', 'press', 'use'].filter((other) => other !== input)) {
      assert.throws(() => world.input(kind, x, 65, z, 0), new RegExp(`IllegalArgumentException: No ${kind} input at ${x}, 65, ${z}`), `${key} takes no ${kind}`);
    }
    if (input) {
      named[input] += 1;
      assert.strictEqual(world.click(x, 65, z, 0), input, key);
    } else assert.strictEqual(world.click(x, 65, z, 0), null, key);
  });
  assert.strictEqual(named.toggle, 1, 'the lever');
  assert.ok(named.press >= 13, 'the buttons');
  assert.ok(named.use >= 50, 'doors, trapdoors, gates and the redstone blocks');
});
