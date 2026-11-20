'use strict';

// What test/core-survive.test.js reads of Minecraft's block table through
// today's server/minecraft-facts.js: every state's name as a setblock
// writes it, whether Minecraft says it survives alone (McbenchEngine.alone)
// and the directions a full block holds it from, as one JSON object on
// stdout. Also the default settle's ticks (server/minecraft-dimension.js),
// for settle-ticks-minecraft.test.js.
//
//   node survive-facts.js <blocks.txt>

const fs = require('fs');
const path = require('path');
const root = path.join(__dirname, '..', '..', '..', '..', '..');
const { MinecraftFacts } = require(path.join(root, 'server', 'minecraft-facts'));
const { DEFAULT_SETTLE_TICKS } = require(path.join(root, 'server', 'minecraft-dimension'));

const facts = new MinecraftFacts(fs.readFileSync(process.argv[2], 'utf8'));
const names = new Array(facts.count);
for (const [key, block] of facts.blocks) {
  for (const [indices, id] of block.states) {
    const values = indices === '-' ? [] : indices.split(',').map(Number);
    names[id] = values.length ? `${key}[${block.properties.map(([name, options], i) => `${name}=${options[values[i]]}`).join(',')}]` : key;
  }
}
process.stdout.write(JSON.stringify({
  names,
  survives: Array.from({ length: facts.count }, (_, state) => Boolean(facts.survives[state])),
  held: Array.from({ length: facts.count }, (_, state) => facts.held[state] || 0),
  defaultSettleTicks: DEFAULT_SETTLE_TICKS,
}));
