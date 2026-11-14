'use strict';

const { BlockStates } = require('../../server/block-states');

// A block table in McbenchBlockTable.java's format, of states written for a
// test (not Minecraft's: what is under test is the code's handling of a
// table). `states`, by block, a block's first state its default:
//   { name: 'test:slab[type=bottom]', emission, lightBlock, full, faces }
// emission and lightBlock 0 unless given; `full`, the full faces as bits over
// down, up, north, south, west, east (0 unless given); `faces`, the six face
// shapes light is tested through (all 0, the empty shape, unless given).
// Face shape 1 is full; the shapes past it are partial, and of those only
// the ordered pairs in `pairs` close a face together.
function blockTable(states, { shapes = 2, pairs = [] } = {}) {
  const lines = [];
  let block = null;
  for (const { name, emission = 0, lightBlock = 0, full = 0, faces = [0, 0, 0, 0, 0, 0] } of states) {
    const id = name.split('[')[0];
    if (id !== block) lines.push(`default\t${id}\t${name}`);
    block = id;
    lines.push([name, emission, lightBlock, full, faces.join(',')].join('\t'));
  }
  const closes = (a, b) => a === 1 || b === 1 || pairs.some(([p, q]) => p === a && q === b);
  let bits = '';
  for (let a = 0; a < shapes; a++) for (let b = 0; b < shapes; b++) bits += closes(a, b) ? '1' : '0';
  lines.push(`faceoccludes\t${shapes}\t${bits}`);
  return `${lines.join('\n')}\n`;
}

// The BlockStates (server/block-states.js) of such a table, air first.
function testBlockStates(states, options) {
  return new BlockStates(blockTable([{ name: 'minecraft:air' }, ...states], options));
}

module.exports = { blockTable, testBlockStates };
