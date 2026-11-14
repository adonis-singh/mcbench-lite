'use strict';

const fs = require('fs');

const { blockStates } = require('./block-states');

// The colours Minecraft's client tints each block state with in the world
// the app builds in (McbenchClient.java's tints file), by state id (the
// block table's order, line for line), each 0xRRGGBB: `world`, by state
// and tint index (`indices` of them), the colour of its faces of that tint
// index; `particle` the colour its breaking particles, the texture a
// block's icon shows (block-icon.js), are multiplied by. see_build's page
// tints its faces with `world` (`bytes`, read by web/viewer.js).
class BlockTints {
  // `text`: the tints file; `states`: the block table's BlockStates.
  constructor(text, states) {
    const lines = text.split('\n').filter(Boolean);
    const [, indices] = lines.shift().split('\t');
    this.indices = Number(indices);
    if (lines.length !== states.count) throw new Error(`The tints have ${lines.length} states, the block table ${states.count}`);
    this.world = new Uint32Array(states.count * this.indices);
    this.particle = new Uint32Array(states.count);
    lines.forEach((line, id) => {
      const [name, world, , particle] = line.split('\t');
      if (states.ids.get(name) !== id) throw new Error(`The tints name state ${id} ${name}, which the block table does not`);
      world.split(',').forEach((color, index) => { this.world[id * this.indices + index] = parseInt(color, 16); });
      this.particle[id] = parseInt(particle, 16);
    });
  }

  // What the page reads: u32 states, u32 tint indices, then by state and
  // tint index u32 0xRRGGBB.
  bytes() {
    this.bytesMade ||= Buffer.concat([Buffer.from(Uint32Array.of(this.world.length / this.indices, this.indices).buffer), Buffer.from(this.world.buffer)]);
    return this.bytesMade;
  }
}

// The tints, read once from the file the first start makes with the
// viewer's assets (minecraft-live.js blockTints). A read that failed is
// tried again when next asked.
let loaded = null;
function blockTints() {
  const { minecraftLive } = require('./minecraft-live');
  loaded ||= Promise.all([minecraftLive.blockTints(), blockStates()])
    .then(([file, states]) => new BlockTints(fs.readFileSync(file, 'utf8'), states))
    .catch((error) => {
      loaded = null;
      throw error;
    });
  return loaded;
}

module.exports = { BlockTints, blockTints };
