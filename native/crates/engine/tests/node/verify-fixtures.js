'use strict';

// The VERIFY requests mcbench-engine's tests run, made by today's verifier
// (server/minecraft-verify.js EngineVerifier) as it writes them for the
// engine: each world and test below goes through EngineVerifier.run with an
// engine that keeps request.json and commands.txt in <out>/<name>/ and
// answers nothing more. Old data versions are fixed by the engine program
// server/engine.js finds (MCBENCH_TOOLS_DIR).
//
//   node verify-fixtures.js <out folder>

const fs = require('fs');
const os = require('os');
const path = require('path');
const root = path.join(__dirname, '..', '..', '..', '..', '..');
const { EngineVerifier } = require(path.join(root, 'server', 'minecraft-verify'));
const { World } = require(path.join(root, 'server', 'world'));

const out = process.argv[2];

function world(blocks) {
  const w = new World();
  for (const [x, y, z, id, states] of blocks) w.set(x, y, z, id, states || {});
  w.finalize();
  return w;
}

// import-data-version.test.js: short grass on dirt and the renamed blocks
// alone, as written at `version`.
const CASES = [
  { version: 2566, blocks: [['minecraft:grass_path', {}], ['minecraft:grass', {}], ['minecraft:chain', { axis: 'x' }]] },
  { version: 3218, blocks: [['minecraft:grass', {}], ['minecraft:chain', { axis: 'x' }]] },
  { version: 3465, blocks: [['minecraft:grass', {}], ['minecraft:chain', { axis: 'y', waterlogged: 'false' }]] },
  { version: 4189, blocks: [['minecraft:short_grass', {}], ['minecraft:chain', { axis: 'z' }]] },
];

const STONE = [[0, 0, 0, 'minecraft:stone']];
// Clocks that keep ticking: 64 pairs of observers facing each other, on stone.
const CLOCK = Array.from({ length: 64 }, (_, i) => [[0, 0, 2 * i, 'minecraft:stone'], [1, 0, 2 * i, 'minecraft:stone'], [0, 1, 2 * i, 'minecraft:observer', { facing: 'east' }], [1, 1, 2 * i, 'minecraft:observer', { facing: 'west' }]]).flat();
// core-engine.test.js: a resin clump on the floor, which the port refuses.
const RESIN = [[0, 0, 0, 'minecraft:stone'], [0, 1, 0, 'minecraft:resin_clump', { down: 'true' }]];

const fixtures = [
  { name: 'stone', world: world(STONE), args: { probes: [[0, 0, 0]] } },
  { name: 'missing-inventory', world: world(STONE), args: { probes: [[0, 0, 0]], inventory_probes: [[0, 0, 0]], inventory_missing: 'error' } },
  { name: 'clock', world: world(CLOCK), args: { probes: [[0, 1, 0]], samples: [0, 70000] } },
  { name: 'resin', world: world(RESIN), args: { probes: [[0, 0, 0]] } },
  ...CASES.map(({ version, blocks }) => ({
    name: `version-${version}`,
    world: world(blocks.flatMap(([id, states], i) => [[3 * i, 0, 0, 'minecraft:dirt'], [3 * i, 1, 0, id, states]])),
    args: { probes: blocks.map((_, i) => [3 * i, 1, 0]), samples: [0, 20], block_data_version: version },
  })),
];

class Kept extends Error {}

async function main() {
  for (const { name, world: w, args } of fixtures) {
    const folder = path.join(out, name);
    fs.mkdirSync(folder, { recursive: true });
    const engine = {
      async warm() {},
      async stop() {},
      async verify(requestFile, commandsFile) {
        fs.copyFileSync(requestFile, path.join(folder, 'request.json'));
        fs.copyFileSync(commandsFile, path.join(folder, 'commands.txt'));
        throw new Kept('kept');
      },
    };
    const verifier = new EngineVerifier({ root: fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-fixture-')), engine });
    await verifier.run(w, args).then(() => { throw new Error(`${name}: the verifier did not reach the engine`); }, (error) => {
      if (!/kept/.test(error.message)) throw error;
    });
    await verifier.stop();
    fs.rmSync(verifier.root, { recursive: true, force: true });
  }
  console.log(fixtures.map(({ name }) => name).join('\n'));
}

main().catch((error) => { console.error(error); process.exit(1); });
