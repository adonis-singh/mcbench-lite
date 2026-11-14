'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const readline = require('node:readline');
const { buildCore } = require('../server/engine');
const { tablesFolder } = require('../server/minecraft-tables');
const { MinecraftFacts } = require('../server/minecraft-facts');
const { blueprintWorld } = require('../server/blueprint');
const { MC_VERSION } = require('../server/assets');

const TABLE = path.join(tablesFolder(MC_VERSION), 'blocks.txt');
const STEPS = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

// The native engine's SURVIVE (core/src/main.rs) against Minecraft's
// canSurvive as the engine's table holds it (McbenchEngine.alone): every
// state of the table placed by the import alone, four blocks from the next,
// and once more with a block of stone where the table says a full block holds
// it. Every state the placement keeps is named exactly when Minecraft says
// it cannot survive there.
test('the engine names the blocks that cannot survive where they are, as Minecraft does, for every state alone and held', { skip: !fs.existsSync(TABLE) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)', timeout: 600000 }, async (t) => {
  const facts = new MinecraftFacts(fs.readFileSync(TABLE, 'utf8'));
  const names = new Array(facts.count);
  for (const [key, block] of facts.blocks) {
    for (const [indices, id] of block.states) {
      const values = indices === '-' ? [] : indices.split(',').map(Number);
      names[id] = values.length ? `${key}[${block.properties.map(([name, options], i) => `${name}=${options[values[i]]}`).join(',')}]` : key;
    }
  }
  const binary = await buildCore();
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-survive-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const file = (name) => path.join(folder, name);
  const child = spawn(binary, [], { cwd: folder, stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => child.kill());
  const lines = readline.createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const next = async () => (await lines.next()).value;
  assert.strictEqual(await next(), 'MCBENCH_ENGINE_READY');
  let id = 0;
  const ask = async (line) => { child.stdin.write(`${line}\n`); return next(); };
  // Places `scenes` ({ state, at, stone }) afresh; answers the positions kept
  // with their state's block, and those SURVIVE names. A block the port
  // refuses to place is left out, and the rest placed again.
  const survive = async (scenes) => {
    const refused = new Set();
    for (;;) {
      const kept = scenes.filter((scene) => !refused.has(names[scene.state].split('[')[0]));
      fs.writeFileSync(file('commands.txt'), kept.map(({ state, at, stone }) => `${stone ? `setblock ${stone.join(' ')} minecraft:stone\n` : ''}setblock ${at.join(' ')} ${names[state]}\n`).join(''));
      const placed = await ask(`PLACE ${++id} ${file('commands.txt')} ${file('blueprint.bin')} fresh`);
      const block = /unported: .*\((minecraft:[a-z0-9_]+)/.exec(placed)?.[1];
      if (block) {
        refused.add(block);
        continue;
      }
      assert.match(placed, /^MCBENCH_PLACED/);
      assert.match(await ask(`SURVIVE ${++id} ${file('unsupported.txt')}`), /^MCBENCH_DONE/);
      const world = blueprintWorld(fs.readFileSync(file('blueprint.bin')), 0);
      const named = new Set(fs.readFileSync(file('unsupported.txt'), 'utf8').split('\n').filter(Boolean));
      return { kept: kept.filter((scene) => world.get(...scene.at)?.id === names[scene.state].split('[')[0]), named, refused };
    }
  };
  const at = (i) => [4 * (i % 256), 64, 4 * Math.floor(i / 256)];
  const alone = await survive(names.map((_, state) => ({ state, at: at(state) })));
  const wrong = alone.kept.filter(({ state, at: pos }) => alone.named.has(pos.join(' ')) !== !facts.survives[state]);
  assert.deepStrictEqual(wrong.map(({ state }) => names[state]).slice(0, 20), [], `${wrong.length} states alone`);
  const holdable = names.map((_, state) => state).filter((state) => facts.held[state]);
  const held = await survive(holdable.map((state, i) => {
    const pos = at(i);
    const direction = Math.log2(facts.held[state] & -facts.held[state]);
    return { state, at: pos, stone: pos.map((v, k) => v + STEPS[direction][k]) };
  }));
  const named = held.kept.filter(({ at: pos }) => held.named.has(pos.join(' ')));
  assert.deepStrictEqual(named.map(({ state }) => names[state]).slice(0, 20), [], `${named.length} held states`);
  assert.ok(alone.kept.length > 25000 && held.kept.length > 4000, `${alone.kept.length} states alone, ${held.kept.length} held`);
  assert.ok(alone.named.size > 3000, `${alone.named.size} named alone`);
});

