'use strict';

// A stand-in for the Rust engine (server/engine.js coreEngine) so tests can run
// build programs and settles without Minecraft: it applies a placement's
// setblock and fill commands to a block map, lets `FakeEngines.rules` change
// the map the way Minecraft would during the ticks, and writes the blueprint
// the engine writes. Install it before server/shot-world.js loads.

const fs = require('node:fs');
const { blocksBlueprint } = require('../../server/blueprint');

class FakeEngines {
  // rules(blocks): edits the "x,y,z" -> spec map once for each tick.
  static rules = () => {};

  static places = [];

  // The ticks of each TICK request.
  static ticks = [];

  // The most placements and ticks that ran at once, and a pause (ms) in each placement.
  static running = 0;

  static peak = 0;

  static delay = 0;

  // The engine instances made.
  static made = [];

  static failures = 0;

  // Warm-ups that fail before one works.
  static warmFailures = 0;

  // Placements whose engine process dies under them (it ran out of memory).
  static deaths = 0;

  constructor() {
    this.blocks = new Map();
    // The engine process, as Engine holds it: null once it stopped.
    this.child = {};
    FakeEngines.made.push(this);
  }

  // As Engine.warm: one start while it runs or after it worked; a failed
  // start (a moment later, as a real one fails) leaves the engine dead, and
  // the next warm starts it again.
  warm(options) {
    if (this.child && this.warming) return this.warming;
    this.warming = (async () => {
      await new Promise((resolve) => setImmediate(resolve));
      if (FakeEngines.warmFailures > 0) {
        FakeEngines.warmFailures -= 1;
        this.child = null;
        throw new Error('The engine stopped (code 1): fixture');
      }
      this.child = {};
      this.options = options;
    })();
    return this.warming;
  }

  get alive() { return this.child !== null; }

  async place(commandsFile, blueprintFile, { fresh }) {
    if (FakeEngines.failures > 0) {
      FakeEngines.failures -= 1;
      throw new Error('The engine failed: fixture');
    }
    if (FakeEngines.deaths > 0) {
      FakeEngines.deaths -= 1;
      this.child = null;
      throw new Error('The engine stopped (SIGKILL): fixture');
    }
    FakeEngines.running += 1;
    FakeEngines.peak = Math.max(FakeEngines.peak, FakeEngines.running);
    if (FakeEngines.delay) await new Promise((resolve) => setTimeout(resolve, FakeEngines.delay));
    FakeEngines.running -= 1;
    if (fresh) {
      this.blocks = new Map();
      FakeEngines.places.push(fs.readFileSync(commandsFile, 'utf8'));
    }
    for (const line of fs.readFileSync(commandsFile, 'utf8').split('\n').filter(Boolean)) {
      const [kind, ...rest] = line.split(' ');
      const coordinates = rest.slice(0, kind === 'fill' ? 6 : 3).map(Number);
      const spec = rest.slice(kind === 'fill' ? 6 : 3).join(' ');
      const [x1, y1, z1, x2 = x1, y2 = y1, z2 = z1] = coordinates;
      for (let x = x1; x <= x2; x += 1) for (let y = y1; y <= y2; y += 1) for (let z = z1; z <= z2; z += 1) {
        if (spec === 'minecraft:air') this.blocks.delete(`${x},${y},${z}`);
        else this.blocks.set(`${x},${y},${z}`, spec);
      }
    }
    fs.writeFileSync(blueprintFile, blocksBlueprint([...this.blocks].map(([key, spec]) => [...key.split(',').map(Number), spec])));
  }

  async tick(ticks) {
    FakeEngines.ticks.push(ticks);
    for (let i = 0; i < ticks; i += 1) FakeEngines.rules(this.blocks);
  }

  async stop() { this.child = null; }
}

// Installs FakeEngines as the Rust engine, and a Paper that needs no
// download. Returns the class.
function installFakeEngines() {
  require('../../server/engine').coreEngine = () => new FakeEngines();
  require('../../server/minecraft-live').minecraftLive.ensurePaper = async () => [];
  return FakeEngines;
}

module.exports = { FakeEngines, installFakeEngines };
