'use strict';

// The block id at each position of a blueprint, as today's
// server/blueprint.js blueprintWorld reads it (test/core-survive.test.js).
// Reads the positions as JSON lines [x, y, z] on stdin; writes the id (or
// "air") a line each.
//
//   node blueprint-ids.js <blueprint file> < positions

const fs = require('fs');
const path = require('path');
const root = path.join(__dirname, '..', '..', '..', '..', '..');
const { blueprintWorld } = require(path.join(root, 'server', 'blueprint'));

const world = blueprintWorld(fs.readFileSync(process.argv[2]), 0);
const out = [];
for (const line of fs.readFileSync(0, 'utf8').split('\n').filter(Boolean)) {
  out.push(world.get(...JSON.parse(line))?.id ?? 'air');
}
process.stdout.write(`${out.join('\n')}\n`);
