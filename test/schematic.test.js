'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-schematic-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTLE_ENGINES = '1';
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const { World } = require('../server/world');
const { writeSchematic, buildSchematic } = require('../server/schematic');
const { stopShotEngines } = require('../server/shot-world');

test.after(async () => { await stopShotEngines(); fs.rmSync(dataDir, { recursive: true, force: true }); });
const { PYTHON, nbtlibMissing } = require('./helpers/python');

const skip = nbtlibMissing();

// Decoded by nbtlib, independently of the writer: dimensions, palette and
// the block at every requested position.
function readBack(bytes, positions) {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'schem-')), 'build.schem');
  fs.writeFileSync(file, bytes);
  const script = `
import json, sys, nbtlib
s = nbtlib.load(sys.argv[1])['Schematic']
w, h, l = int(s['Width']), int(s['Height']), int(s['Length'])
palette = {int(v): k for k, v in s['Blocks']['Palette'].items()}
data = bytes(b & 0xff for b in s['Blocks']['Data'])
cells, value, shift = [], 0, 0
for b in data:
    value |= (b & 0x7f) << shift
    shift += 7
    if not b & 0x80:
        cells.append(value); value, shift = 0, 0
at = lambda x, y, z: palette[cells[x + z * w + y * w * l]]
print(json.dumps({
    'version': int(s['Version']), 'dataVersion': int(s['DataVersion']),
    'size': [w, h, l], 'cells': len(cells), 'palette': len(palette),
    'blocks': [at(*p) for p in json.loads(sys.argv[2])],
}))`;
  return JSON.parse(execFileSync(PYTHON, ['-c', script, file, JSON.stringify(positions)], { encoding: 'utf8' }));
}

test('schematic keeps every authored block state at its position', { skip }, () => {
  const world = new World();
  world.set(-2, -1, 3, 'minecraft:stone');
  world.set(1, 4, 5, 'minecraft:oak_stairs', { half: 'top', facing: 'east' });
  world.fill([0, 0, 3], [1, 0, 3], 'minecraft:oak_fence');
  const result = readBack(writeSchematic(world), [[0, 0, 0], [3, 5, 2], [2, 1, 0], [3, 1, 0], [1, 2, 1]]);
  assert.equal(result.version, 3);
  assert.equal(result.dataVersion, 4556);
  assert.deepEqual(result.size, [4, 6, 3]);
  assert.equal(result.cells, 4 * 6 * 3);
  assert.deepEqual(result.blocks, [
    'minecraft:stone',
    'minecraft:oak_stairs[facing=east,half=top]',
    'minecraft:oak_fence',
    'minecraft:oak_fence',
    'minecraft:air',
  ]);
});

test('palettes past 128 states use multi-byte indexes', { skip }, () => {
  const world = new World();
  for (let i = 0; i < 300; i += 1) world.set(i, 0, 0, 'minecraft:stone_bricks', { variant: String(i) });
  const result = readBack(writeSchematic(world), [[0, 0, 0], [127, 0, 0], [299, 0, 0]]);
  assert.equal(result.palette, 301);
  assert.equal(result.cells, 300);
  assert.deepEqual(result.blocks, [
    'minecraft:stone_bricks[variant=0]',
    'minecraft:stone_bricks[variant=127]',
    'minecraft:stone_bricks[variant=299]',
  ]);
});

test('an empty build has nothing to export', () => {
  assert.throws(() => writeSchematic(new World()), /empty/);
});

test('a chat\'s schematic is the world the engine holds after placing the build, kept for the next export', { skip }, async () => {
  const world = new World();
  world.fill([0, 0, 0], [2, 0, 0], 'minecraft:glass_pane');
  world.set(0, 1, 0, 'minecraft:torch');
  world.finalize();
  // The placement's updates: the panes join, the torch with nothing under it goes.
  FakeEngines.placing = (blocks) => {
    blocks.set('0,64,0', 'minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=false]');
    blocks.set('1,64,0', 'minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=true]');
    blocks.set('2,64,0', 'minecraft:glass_pane[east=false,north=false,south=false,waterlogged=false,west=true]');
    blocks.delete('0,65,0');
  };
  const places = FakeEngines.places.length;
  const result = readBack(await buildSchematic('chat', world), [[0, 0, 0], [1, 0, 0], [2, 0, 0]]);
  assert.deepEqual(result.size, [3, 1, 1], 'the torch Minecraft did not keep is not in it');
  assert.deepEqual(result.blocks, [
    'minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=false]',
    'minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=true]',
    'minecraft:glass_pane[east=false,north=false,south=false,waterlogged=false,west=true]',
  ]);
  await buildSchematic('chat', world);
  assert.equal(FakeEngines.places.length, places + 1, 'placed once: the second export reads the kept world');
  FakeEngines.placing = () => {};
});

test('a build Minecraft keeps none of has no schematic', async () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:torch');
  world.finalize();
  FakeEngines.placing = (blocks) => blocks.clear();
  await assert.rejects(buildSchematic('gone', world), (error) => error.statusCode === 400 && /keeps none/.test(error.message));
  FakeEngines.placing = () => {};
});
