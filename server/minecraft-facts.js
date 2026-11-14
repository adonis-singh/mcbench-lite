'use strict';

const fs = require('fs');
const path = require('path');

const { tablesFolder } = require('./minecraft-tables');
const { MC_VERSION } = require('./assets');

// Direction.getUnitVec3i, in Direction.ordinal() order.
const STEPS = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

// What the server asks of Minecraft that the Java engine's dump holds
// (core/minecraft/<version>/blocks.txt, McbenchEngine.dump): its blocks, their
// classes and states, its item tags, what each block state placed alone in an empty level does,
// and where a full block holds one that cannot survive alone
// (McbenchEngine.alone), by state id.
class MinecraftFacts {
  constructor(text) {
    this.blocks = new Map(); // block id -> { className, supertypes, defaultState, properties: [[name, values]], states: indices -> state id, defaults: indices }
    this.itemTags = new Map();
    let block = null;
    let alone;
    let held;
    for (const line of text.split('\n')) {
      const tab = line.indexOf('\t');
      const type = line.slice(0, tab);
      if (type === 'block') {
        const [, key, className, , , , defaultState, , , , , supertypes] = line.split('\t');
        block = { className: className.slice(className.lastIndexOf('.') + 1), supertypes: supertypes === '-' ? [] : supertypes.split(','), defaultState: Number(defaultState), properties: [], states: new Map(), defaults: null };
        this.blocks.set(key, block);
      } else if (type === 'property') {
        const [, name, values] = line.split('\t');
        block.properties.push([name, values.split(',')]);
      } else if (type === 'state') {
        const [, id, indices, flags] = line.split('\t');
        block.states.set(indices, Number(id));
        if (Number(id) === block.defaultState) {
          block.defaults = indices === '-' ? [] : indices.split(',').map(Number);
          // Flag 1: BlockState.isAir, which every state of an air block has.
          block.air = (Number(flags) & 1) === 1;
        }
      } else if (type === 'itemtag') {
        const [, name, members] = line.split('\t');
        this.itemTags.set(name, new Set(members === '-' ? [] : members.split(',')));
      } else if (type === 'alone') alone = line.slice(tab + 1);
      else if (type === 'held') held = line.slice(tab + 1);
    }
    this.alone = Uint8Array.from(alone.split(','), (bits) => parseInt(bits, 16));
    this.count = this.alone.length;
    // By state, whether it survives alone (canSurvive in an empty level),
    // and for one that does not, the directions (bits over
    // Direction.ordinal()) in which a block of stone alone lets it survive.
    this.survives = new Uint8Array(this.count).fill(1);
    this.held = new Uint8Array(this.count);
    for (const entry of held.split(',').filter(Boolean)) {
      const [state, directions] = entry.split(':');
      this.survives[Number(state)] = 0;
      this.held[Number(state)] = Number(directions);
    }
    this.palettes = new WeakMap();
  }

  // The state id of block `id` with `states`, its block's defaults where it
  // names none.
  stateId(id, states = {}) {
    const block = this.blocks.get(id);
    if (!block) throw new Error(`Minecraft has no block ${id}`);
    const indices = [...block.defaults];
    for (const [name, value] of Object.entries(states)) {
      const at = block.properties.findIndex(([property]) => property === name);
      const index = at < 0 ? -1 : block.properties[at][1].indexOf(String(value));
      if (index < 0) throw new Error(`Minecraft has no block state ${id}[${name}=${value}]`);
      indices[at] = index;
    }
    return block.states.get(indices.length ? indices.join(',') : '-');
  }

  // The simple name of the class of block `id` (BedBlock, LeverBlock...).
  className(id) {
    const block = this.blocks.get(id);
    if (!block) throw new Error(`Minecraft has no block ${id}`);
    return block.className;
  }

  // Whether block `id` is air to Minecraft (air, cave air, void air).
  isAir(id) {
    const block = this.blocks.get(id);
    if (!block) throw new Error(`Minecraft has no block ${id}`);
    return block.air;
  }

  // Whether block `id` is of Minecraft's class `className`: its own class or
  // a superclass below Block (the dump's supertypes).
  is(id, className) {
    const block = this.blocks.get(id);
    if (!block) throw new Error(`Minecraft has no block ${id}`);
    return block.className === className || block.supertypes.includes(className);
  }

  // The items of the item tag `name`.
  itemTag(name) {
    const members = this.itemTags.get(name);
    if (!members) throw new Error(`Minecraft has no item tag ${name}`);
    return members;
  }

  // The properties of block `id`'s default state, by name.
  defaults(id) {
    const block = this.blocks.get(id);
    if (!block) throw new Error(`Minecraft has no block ${id}`);
    return Object.fromEntries(block.properties.map(([name, values], i) => [name, values[block.defaults[i]]]));
  }

  // Where a full block, alone beside block `id` with `states`, lets it
  // survive when it cannot alone: the offsets of those neighbours, in
  // Direction order (down, up, north, south, west, east).
  heldBy(id, states) {
    const directions = this.held[this.stateId(id, states)];
    return STEPS.filter((_, direction) => (directions >> direction) & 1);
  }

  // For each entry of a world's palette (server/world.js PaletteStore, entry 0
  // air), 1 when its state would not stay as placed alone (alone's bits: the
  // import's last pass); made once for each entry.
  lastPass(palette) {
    let known = this.palettes.get(palette);
    const size = palette.entries.length;
    if (!known || known.length < size) {
      const last = new Uint8Array(size);
      if (known) last.set(known);
      for (let index = known ? known.length : 1; index < size; index += 1) {
        const entry = palette.entries[index];
        last[index] = this.alone[this.stateId(entry.id, entry.states)] ? 1 : 0;
      }
      known = last;
      this.palettes.set(palette, known);
    }
    return known;
  }
}

// The facts of the pinned Minecraft, read once from the engine's table, which
// the first start or `node scripts/prepare-minecraft.js` makes.
let loaded = null;
function minecraftFacts() {
  loaded ||= new MinecraftFacts(fs.readFileSync(path.join(tablesFolder(MC_VERSION), 'blocks.txt'), 'utf8'));
  return loaded;
}

module.exports = { MinecraftFacts, minecraftFacts };
