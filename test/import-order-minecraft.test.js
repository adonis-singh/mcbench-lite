'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { build } = require('../scripts/build-wasm');
const { tablesFolder } = require('../server/minecraft-tables');
const { MinecraftFacts } = require('../server/minecraft-facts');
const { blueprintWorld } = require('../server/blueprint');
const { MC_VERSION } = require('../server/assets');

// The import's order (server/live-placement.js) against the engine, which is
// Minecraft's code: which block states stay as placed alone, as the engine's
// table says (McbenchEngine.alone), and that the import gives back the
// blocks that do not with what holds them, for every state of the table.
const TABLE = path.join(tablesFolder(MC_VERSION), 'blocks.txt');
const skip = !fs.existsSync(TABLE) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)';

let wasm;
let CoreWorld;
let facts;
test.before(async () => {
  if (skip) return;
  ({ CoreWorld } = await import('../web/core-world.mjs'));
  wasm = await WebAssembly.compile(fs.readFileSync(await build()));
  facts = new MinecraftFacts(fs.readFileSync(TABLE, 'utf8'));
});

// Every block state of the table as a command's block: its id and every
// property, by state id.
function specs() {
  const all = new Array(facts.count);
  for (const [key, block] of facts.blocks) {
    for (const [indices, id] of block.states) {
      const values = indices === '-' ? [] : indices.split(',').map(Number);
      all[id] = values.length ? `${key}[${block.properties.map(([name, options], i) => `${name}=${options[values[i]]}`).join(',')}]` : key;
    }
  }
  return all;
}

// Places `place(i)` for each of `count` items on one world, leaving out the
// blocks whose code the engine refuses (it does not model them), and answers
// { world, refused }.
function placeAll(count, lines, after) {
  const world = new CoreWorld(wasm);
  const refused = new Set();
  for (;;) {
    try {
      const kept = [];
      for (let i = 0; i < count; i += 1) if (!refused.has(i)) kept.push(i);
      world.place(kept.map((i) => lines(i).placed).join(''));
      for (const step of after) world.place(kept.map((i) => step(i)).join(''), { fresh: false });
      return { world, kept, refused };
    } catch (error) {
      const block = /^The engine failed: unported: .*\((minecraft:[a-z0-9_]+)/.exec(error.message)?.[1];
      if (!block) throw error;
      for (let i = 0; i < count; i += 1) if (lines(i).key === block) refused.add(i);
    }
  }
}

const AIR = new Set(['minecraft:air', 'minecraft:cave_air', 'minecraft:void_air']);
const STEPS = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

// Each state alone, four blocks from the next, placed by the import, then a
// block of stone set and removed again beside it on each of its six sides in
// turn, each a command of its own with the import's updates: its block stays
// where it was exactly when the table names neither its placement, nor shape
// updates, nor neighbour updates among what changes it (bits 1, 2 and 4;
// the ticks those leave, bits 8 and 16, are the engine's own test against
// the table, core/src/engine.rs).
test('a block state stays through its placement and its neighbours\' updates exactly when the engine\'s table says so', { skip, timeout: 600000 }, () => {
  const all = specs();
  const side = Math.ceil(Math.sqrt(all.length));
  const at = (i) => [4 * (i % side), 64, 4 * Math.floor(i / side)];
  const lines = (i) => ({ key: all[i].split('[')[0], placed: `setblock ${at(i).join(' ')} ${all[i]}\n` });
  const after = STEPS.flatMap(([dx, dy, dz]) => ['minecraft:stone', 'minecraft:air'].map((block) => (i) => {
    const [x, y, z] = at(i);
    return `setblock ${x + dx} ${y + dy} ${z + dz} ${block}\n`;
  }));
  const { world, kept, refused } = placeAll(all.length, lines, after);
  const placed = blueprintWorld(Buffer.from(world.place('', { fresh: false })), 0);
  assert.ok(refused.size < 400, `the engine refuses ${refused.size} states`);
  const wrong = [];
  for (const i of kept) {
    // The blueprint holds no air: an air state that stays is nothing there.
    const stays = (placed.get(...at(i))?.id ?? null) === (AIR.has(lines(i).key) ? null : lines(i).key);
    if (stays !== ((facts.alone[i] & 7) === 0)) wrong.push(`${all[i]} ${stays ? 'stays' : 'does not stay'}, the table's reasons ${facts.alone[i]}`);
  }
  const byBlock = {}; for (const line of wrong) { const k = line.split(/[[ ]/)[0] + (line.includes(" stays") ? " stays" : " goes"); byBlock[k] = (byBlock[k] || 0) + 1; }
  assert.deepStrictEqual(byBlock, {}, `${wrong.length} states`);
});

// Every state that cannot survive alone and that a full block beside it
// holds (the table's held line), imported with what holds it, where the
// import's order is the hardest on it: what holds it comes after it in the
// order a pass places in (in the next chunk column for a side, above it, or,
// for a support below it, a block of sand, which is of the last pass
// itself), and a block of stone beside it that does not hold it comes after
// it in its pass (before it where every side after it holds it). The engine
// gives back every block of the blueprint, as it does when the states are
// placed after everything else. Put in the first pass instead, the states
// whose neighbours' updates remove them (dust, repeaters, rails, carpets,
// vines...) are lost.
test('every state that a full block holds comes back from the import with what holds it; in the first pass, many would not', { skip, timeout: 600000 }, () => {
  const facts = require('../server/minecraft-facts');
  const { World } = require('../server/world');
  const { importCommands } = require('../server/live-placement');
  const real = new MinecraftFacts(fs.readFileSync(TABLE, 'utf8'));
  const { minecraftFacts } = facts;
  const all = specs();
  const props = (spec) => Object.fromEntries((/\[(.*)\]$/.exec(spec)?.[1] ?? '').split(',').filter(Boolean).map((pair) => pair.split('=')));
  // By direction (Direction order), the scenes: P, what holds it, the stone beside it.
  const held = [];
  for (let i = 0; i < real.count; i += 1) if (real.alone[i] && real.held[i]) held.push(i);
  const scenes = [];
  const next = [0, 0, 0, 0, 0, 0];
  for (const i of held) {
    const d = Math.log2(real.held[i] & -real.held[i]);
    const n = next[d]++;
    // Each direction has its own chunk columns, 256 scenes a column, four blocks apart.
    const base = [64 * d, 0, 64 * Math.floor(n / 256)];
    const y = -62 + 4 * (n % 256);
    const at = d === 4 ? [base[0], y, base[2] + 8] : d === 5 ? [base[0] + 15, y, base[2] + 8] : d === 2 ? [base[0] + 8, y, base[2]] : d === 3 ? [base[0] + 8, y, base[2] + 15] : [base[0] + 8, y, base[2] + 8];
    const step = STEPS[d];
    const support = at.map((v, k) => v + step[k]);
    const free = [[0, 0, 1, 3], [1, 0, 0, 5], [0, 1, 0, 1], [0, 0, -1, 2], [-1, 0, 0, 4], [0, -1, 0, 0]].find(([, , , dir]) => !((real.held[i] >> dir) & 1) && dir !== d && (dir !== 3 || (at[2] & 15) < 15) && (dir !== 5 || (at[0] & 15) < 15) && (dir !== 2 || (at[2] & 15) > 0) && (dir !== 4 || (at[0] & 15) > 0));
    scenes.push({ i, at, support, poke: at.map((v, k) => v + free[k]), holder: d === 0 ? 'minecraft:sand' : 'minecraft:stone' });
  }
  const run = (kept, lastPass) => {
    const world = new World();
    for (const s of kept) world.set(...s.at, all[s.i].split('[')[0], props(all[s.i]));
    for (const s of kept) world.set(...s.poke, 'minecraft:stone');
    for (const s of kept) world.set(...s.support, s.holder);
    world.finalize();
    facts.minecraftFacts = () => ({ lastPass });
    const { commands } = importCommands(world, null, 0);
    const engine = new CoreWorld(wasm);
    const placed = blueprintWorld(Buffer.from(engine.place(commands.map((c) => `${c}\n`).join(''))), 0);
    return kept.filter((s) => placed.get(...s.at)?.id !== all[s.i].split('[')[0] || placed.get(...s.support)?.id !== s.holder || placed.get(...s.poke)?.id !== 'minecraft:stone');
  };
  // The order that is kindest to each state: everything else first, then the
  // states. A state lost even so needs more than one full block (kelp's body
  // its head above, an ascending rail the block on its high side).
  const statesLast = (palette) => Uint8Array.from(palette.entries, (entry) => (entry && entry.id !== 'minecraft:sand' && entry.id !== 'minecraft:stone' ? 1 : 0));
  let kept = scenes;
  let needMore;
  const refused = new Set();
  for (;;) {
    try {
      needMore = new Set(run(kept, statesLast));
      break;
    } catch (error) {
      const block = /^The engine failed: unported: .*\((minecraft:[a-z0-9_]+)/.exec(error.message)?.[1];
      if (!block) throw error;
      refused.add(block);
      kept = scenes.filter((s) => !refused.has(all[s.i].split('[')[0]));
    }
  }
  kept = kept.filter((s) => !needMore.has(s));
  assert.ok(kept.length > 5000, `${kept.length} states`);
  const lostInOrder = run(kept, (palette) => real.lastPass(palette));
  assert.deepStrictEqual(lostInOrder.map((s) => all[s.i]), [], `${lostInOrder.length} of ${kept.length} states are lost from the import`);
  // The same scenes with their states put in the first pass.
  const lost = run(kept, (palette) => Uint8Array.from(palette.entries, (entry) => (entry?.id === 'minecraft:sand' ? 1 : 0)));
  facts.minecraftFacts = minecraftFacts;
  const lostBlocks = new Set(lost.map((s) => all[s.i].split('[')[0]));
  console.log(`# ${kept.length} states; ${needMore.size} need more than a full block, ${refused.size} blocks are refused; in the first pass ${lost.length} states of ${lostBlocks.size} blocks are lost: ${[...lostBlocks].join(' ')}`);
  for (const block of ['minecraft:redstone_wire', 'minecraft:repeater', 'minecraft:comparator', 'minecraft:rail', 'minecraft:vine', 'minecraft:white_carpet']) assert.ok(lostBlocks.has(block), `${block} is lost in the first pass`);
});
