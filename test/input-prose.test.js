'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { build } = require('../scripts/build-wasm');
const { tablesFolder } = require('../server/minecraft-tables');
const { MinecraftFacts } = require('../server/minecraft-facts');
const { MC_VERSION } = require('../server/assets');

const TABLE = path.join(tablesFolder(MC_VERSION), 'blocks.txt');
const ROOT = path.join(__dirname, '..');
const read = (file) => fs.readFileSync(path.join(ROOT, file), 'utf8');

// The words the prose names blocks by, and Minecraft's class of each: how
// this test reads the sentences, not what the engine knows.
const NOUNS = {
  'trapdoor': 'TrapDoorBlock', 'door': 'DoorBlock', 'fence gate': 'FenceGateBlock', 'repeater': 'RepeaterBlock',
  'comparator': 'ComparatorBlock', 'note block': 'NoteBlock', 'daylight detector': 'DaylightDetectorBlock',
  'redstone dust': 'RedStoneWireBlock', 'decorated pot': 'DecoratedPotBlock', 'lever': 'LeverBlock', 'button': 'ButtonBlock',
};
// The classes a passage names, by NOUNS (a trapdoor is no door).
function named(passage) {
  let text = passage.toLowerCase();
  const classes = new Set();
  for (const [noun, className] of Object.entries(NOUNS)) {
    if (new RegExp(`\\b${noun}s?\\b`).test(text)) classes.add(className);
    text = text.replace(new RegExp(`\\b${noun}s?\\b`, 'g'), ' ');
  }
  return classes;
}
// The passage of `text` from `from` to `to`, both included.
function passage(text, from, to) {
  const start = text.indexOf(from);
  const end = text.indexOf(to, start);
  assert.ok(start >= 0 && end > start, `no passage from ${JSON.stringify(from)} to ${JSON.stringify(to)}`);
  return text.slice(start, end + to.length);
}

// What the engine names for every block of the table in its default state,
// alone on a floor (CoreWorld.inputs, core/src/blocks/mod.rs input_of), as
// Minecraft's classes (a block's own, or the class of NOUNS it extends, as a
// copper door is a DoorBlock): those of the blocks that take a toggle, a
// press and a use, and the blocks of DoorBlock and TrapDoorBlock that take
// none. Each
// sentence that names what takes which input names exactly those classes,
// and the ones that say iron doors and trapdoors take no use are right.
test('the prose that names what takes a use, a toggle and a press names what the engine takes', { skip: !fs.existsSync(TABLE) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)', timeout: 600000 }, async () => {
  const { CoreWorld } = await import('../web/core-world.mjs');
  const { readBlueprint, InputBlocks } = await import('../web/live-patch.mjs');
  const wasm = await WebAssembly.compile(fs.readFileSync(await build()));
  const facts = new MinecraftFacts(fs.readFileSync(TABLE, 'utf8'));
  const all = [...facts.blocks.keys()];
  const world = new CoreWorld(wasm);
  const refused = new Set();
  let keys;
  let side;
  let index;
  for (;;) {
    keys = all.filter((key) => !refused.has(key));
    side = Math.ceil(Math.sqrt(keys.length));
    const lines = [`fill 0 64 0 ${3 * side} 64 ${3 * side} minecraft:stone\n`];
    keys.forEach((key, i) => lines.push(`setblock ${3 * (i % side)} 65 ${3 * Math.floor(i / side)} ${key}\n`));
    try {
      const blueprint = readBlueprint(world.place(lines.join('')));
      index = new InputBlocks();
      index.load(blueprint, world.inputs());
      break;
    } catch (error) {
      const block = /^The engine failed: unported: .*\((minecraft:[a-z0-9_]+)/.exec(error.message)?.[1];
      if (!block) throw error;
      refused.add(block);
    }
  }
  const takes = { use: new Set(), toggle: new Set(), press: new Set() };
  const unopened = new Set();
  keys.forEach((key, i) => {
    const input = index.at(3 * (i % side), 65, 3 * Math.floor(i / side));
    const className = Object.values(NOUNS).find((noun) => facts.is(key, noun)) ?? facts.className(key);
    if (input) takes[input].add(className);
    else if (className === 'DoorBlock' || className === 'TrapDoorBlock') unopened.add(key);
  });
  assert.deepStrictEqual([...takes.toggle], ['LeverBlock']);
  assert.deepStrictEqual([...takes.press], ['ButtonBlock']);
  assert.deepStrictEqual([...unopened].sort(), ['minecraft:iron_door', 'minecraft:iron_trapdoor'], 'the doors and trapdoors no hand opens');
  const sorted = (classes) => [...classes].sort();
  const contract = read('server/verification-contract.js');
  const uses = {
    'test_build actions': passage(contract, "use is a survival player\\'s right click", 'or decorated pot (wobbles)'),
    'test_build guidance': passage(contract, 'or use for a survival player', 'or wobbles a decorated pot'),
    'Java experiment error': passage(read('server/McbenchExperiment.java'), 'type.equals("use") ? "', 'decorated pot"'),
    'Rust experiment error': passage(read('core/src/verify/experiment.rs'), 'kind == "use" {', 'decorated pot"'),
  };
  for (const [where, text] of Object.entries(uses)) assert.deepStrictEqual(sorted(named(text)), sorted(takes.use), where);
  assert.match(passage(contract, "use is a survival player\\'s right click", 'or decorated pot (wobbles)'), /door or trapdoor \(opens or closes it; iron ones cannot be opened by hand\)/);
  assert.match(contract, /Iron doors and trapdoors cannot be opened by hand/);
  // The guidance and the actions name the button's press and the lever's toggle.
  assert.deepStrictEqual(sorted(named(passage(contract, 'Inputs after settling:', 'toggle a lever'))), ['ButtonBlock', 'LeverBlock']);
  assert.deepStrictEqual(sorted(named(passage(contract, 'type is press for a button', 'toggle for a lever'))), ['ButtonBlock', 'LeverBlock']);
  // The live world: levers pulled, buttons pressed, doors and other blocks used.
  const live = passage(read('server/prompt.js'), 'there the user can pull levers', 'use doors and other blocks');
  assert.deepStrictEqual(sorted(named(live)), ['ButtonBlock', 'DoorBlock', 'LeverBlock']);
  assert.ok(takes.use.has('DoorBlock') && takes.use.size > 1, 'doors and other blocks take a use');
  // The books a lectern is supplied, as the guidance names them, are Minecraft's tag minecraft:lectern_books.
  const books = passage(contract, 'add item (', ')').match(/\b[a-z_]+_book\b/g).map((item) => `minecraft:${item}`);
  assert.deepStrictEqual(books.sort(), [...facts.itemTag('minecraft:lectern_books')].sort());
});
