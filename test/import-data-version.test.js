'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { tablesFolder } = require('../server/minecraft-tables');
const { MC_VERSION, MC_DATA_VERSION } = require('../server/assets');
const { World } = require('../server/world');
const { fixedPalette } = require('../server/engine');
const { importCommands } = require('../server/live-placement');
const { EngineVerifier } = require('../server/minecraft-verify');

const skip = !fs.existsSync(path.join(tablesFolder(MC_VERSION), 'blocks.txt')) && 'needs the engine\'s tables (node scripts/prepare-minecraft.js)';

// Blocks renamed since older versions, the data fixer's own cases (core/src/
// verify/mod.rs, scripts/verifier-fixtures.js block-version/old-chain): chain
// is iron_chain since 1.21.9, grass short_grass since 1.20.3, grass_path
// dirt_path since 1.17. Each world stands short grass on dirt and the rest
// alone, as written at `version`.
const CASES = [
  { version: 2566, blocks: [['minecraft:grass_path', {}, 'minecraft:dirt_path'], ['minecraft:grass', {}, 'minecraft:short_grass'], ['minecraft:chain', { axis: 'x' }, 'minecraft:iron_chain']] },
  { version: 3218, blocks: [['minecraft:grass', {}, 'minecraft:short_grass'], ['minecraft:chain', { axis: 'x' }, 'minecraft:iron_chain']] },
  { version: 3465, blocks: [['minecraft:grass', {}, 'minecraft:short_grass'], ['minecraft:chain', { axis: 'y', waterlogged: 'false' }, 'minecraft:iron_chain']] },
  { version: 4189, blocks: [['minecraft:short_grass', {}, 'minecraft:short_grass'], ['minecraft:chain', { axis: 'z' }, 'minecraft:iron_chain']] },
];
function worldOf(blocks) {
  const world = new World();
  blocks.forEach(([id, states], i) => {
    world.set(3 * i, 0, 0, 'minecraft:dirt');
    world.set(3 * i, 1, 0, id, states);
  });
  world.finalize();
  return world;
}

test('a world written at an older version is ordered by its blocks as the data fixer brings them forward, and keeps them as written', { skip, timeout: 600000 }, async () => {
  for (const { version, blocks } of CASES) {
    const world = worldOf(blocks);
    const placed = await fixedPalette(world.palette, version);
    const fixed = (id, states) => placed.entries[world.palette.entries.findIndex((entry) => entry && entry.id === id && JSON.stringify(entry.states) === JSON.stringify(states))];
    for (const [id, states, now] of blocks) assert.strictEqual(fixed(id, states).id, now, `${id} at ${version}`);
    const { commands } = importCommands(world, null, 64, placed);
    // The commands name the blocks as written; short grass, which needs the
    // dirt under it, comes after every block that stays alone.
    const grass = blocks.find(([, , now]) => now === 'minecraft:short_grass')[0];
    const at = (id) => commands.findIndex((command) => command.endsWith(` ${id}`) || command.includes(` ${id}[`));
    assert.ok(blocks.every(([id]) => at(id) >= 0), `${version}: ${commands.join(' | ')}`);
    for (const [id, , now] of blocks) if (now !== 'minecraft:short_grass') assert.ok(at(id) < at(grass), `${version}: ${id} before ${grass}`);
    assert.ok(at('minecraft:dirt') < at(grass));
  }
  // The fixer leaves a world of the pinned version, or a later one, as written
  // (the engine's VERIFY refuses a later one before it places).
  const current = worldOf([['minecraft:iron_chain', { axis: 'x' }]]);
  assert.strictEqual(await fixedPalette(current.palette, MC_DATA_VERSION), current.palette);
  assert.strictEqual(await fixedPalette(current.palette, MC_DATA_VERSION + 1), current.palette);
  // An old name read as the pinned version's is no block Minecraft has.
  assert.throws(() => importCommands(worldOf([['minecraft:grass', {}]]), null, 64), /Minecraft has no block minecraft:grass/);
});

// The verifier imports each world and reads its blocks back: every renamed
// block is there as the fixer names it, and the grass on its dirt.
test('the verifier imports worlds written at older versions, both passes, and reads the fixed blocks back', { skip, timeout: 600000 }, async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-data-version-'));
  const verifier = new EngineVerifier({ root });
  t.after(async () => { await verifier.stop(); fs.rmSync(root, { recursive: true, force: true }); });
  for (const { version, blocks } of CASES) {
    const world = worldOf(blocks);
    const probes = blocks.map((_, i) => [3 * i, 1, 0]);
    const run = await verifier.run(world, { probes, samples: [0, 20], block_data_version: version }, { reportPath: path.join(root, `${version}.json`) });
    const report = JSON.parse(fs.readFileSync(run.reportFile, 'utf8'));
    for (const observation of report.observations) {
      assert.deepStrictEqual(observation.blocks.map(({ block }) => block.split('[')[0]), blocks.map(([, , now]) => now), `${version} at tick ${observation.tick}`);
    }
  }
});
