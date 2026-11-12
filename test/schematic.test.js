'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { World } = require('../server/world');
const { writeSchematic } = require('../server/schematic');
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
