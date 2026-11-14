'use strict';

const { BlockTints } = require('../../server/block-tints');

// A tints file in McbenchClient.java's format for a test's BlockStates (not
// Minecraft's colours: what is under test is the code's handling of them),
// two tint indices: `colors`, by state name, { world: [0xRRGGBB, 0xRRGGBB],
// single, particle, faces }; a state not named is untinted.
function tintsText(states, colors = {}) {
  const hex = (color) => color.toString(16).padStart(6, '0');
  const lines = ['tints\t2'];
  for (const name of states.ids.keys()) {
    const { world = [0xffffff, 0xffffff], single = 0xffffff, particle = 0xffffff, faces = '-' } = colors[name] || {};
    lines.push([name, world.map(hex).join(','), hex(single), hex(particle), faces].join('\t'));
  }
  return `${lines.join('\n')}\n`;
}

function testBlockTints(states, colors) {
  return new BlockTints(tintsText(states, colors), states);
}

module.exports = { tintsText, testBlockTints };
