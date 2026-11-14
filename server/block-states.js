'use strict';

const fs = require('fs');

// What the pinned Minecraft says of each of its block states, read from the
// block table (McbenchBlockTable.java), by state id (the table's order, which
// is Minecraft's registry order, air first): the light it emits, the light
// it blocks, its full faces, and the face shapes Paper's light engine tests
// light through, with whether each pair of face shapes occludes. see_build's
// page lights and culls with these (`bytes`, read by web/viewer.js); the
// world route hides blocks with them and tells the page each palette entry's
// state id (`ofPalette`).
class BlockStates {
  // `text`: the block table's lines.
  constructor(text) {
    const lines = text.split('\n').filter(Boolean);
    const [, shapes, pairs] = lines.pop().split('\t');
    this.shapes = Number(shapes);
    // A face shape's number is kept in a byte.
    if (this.shapes > 256) throw new Error(`The block table has ${this.shapes} face shapes, more than a byte numbers`);
    this.occludes = Uint8Array.from(pairs, (bit) => (bit === '1' ? 1 : 0));
    this.defaults = new Map(); // block id -> its default state's properties
    this.ids = new Map(); // state name, properties sorted -> state id
    const states = [];
    for (const line of lines) {
      const fields = line.split('\t');
      if (fields[0] === 'default') this.defaults.set(fields[1], properties(fields[2]));
      else states.push(fields);
    }
    this.count = states.length;
    this.emission = new Uint8Array(this.count);
    this.lightBlock = new Uint8Array(this.count);
    this.fullFaces = new Uint8Array(this.count);
    this.faces = new Uint8Array(this.count * 6);
    states.forEach(([name, emission, lightBlock, full, faces], id) => {
      this.ids.set(name, id);
      this.emission[id] = Number(emission);
      this.lightBlock[id] = Number(lightBlock);
      this.fullFaces[id] = Number(full);
      faces.split(',').forEach((shape, direction) => { this.faces[id * 6 + direction] = Number(shape); });
    });
    this.palettes = new WeakMap();
  }

  // The state id of block `id` with `states`, its block's defaults where it
  // names none.
  idOf(id, states) {
    const defaults = this.defaults.get(id);
    if (!defaults) throw new Error(`Minecraft has no block ${id}`);
    const merged = Object.entries({ ...defaults, ...states }).sort(([a], [b]) => (a < b ? -1 : 1));
    const name = merged.length ? `${id}[${merged.map(([key, value]) => `${key}=${value}`).join(',')}]` : id;
    const state = this.ids.get(name);
    if (state === undefined) throw new Error(`Minecraft has no block state ${name}`);
    return state;
  }

  // For each entry of a world's palette (server/world.js PaletteStore, entry
  // 0 air), its state id and whether its six faces are full (it hides what is
  // behind each of them, and blocks all light: every such state's light
  // block is 15), made once for each entry.
  ofPalette(palette) {
    let held = this.palettes.get(palette);
    const size = palette.entries.length;
    if (!held || held.size < size) {
      const ids = new Uint16Array(size);
      const occluding = new Uint8Array(size);
      const from = held ? held.size : 1;
      if (held) {
        ids.set(held.ids);
        occluding.set(held.occluding);
      }
      for (let index = from; index < size; index += 1) {
        const entry = palette.entries[index];
        ids[index] = this.idOf(entry.id, entry.states);
        occluding[index] = this.fullFaces[ids[index]] === 63 ? 1 : 0;
      }
      held = { size, ids, occluding };
      this.palettes.set(palette, held);
    }
    return held;
  }

  // What the page reads (web/viewer.js loadResources): u32 states, u32 face
  // shapes, then by state its emission, light block and full faces, its six
  // face shapes, and the face shapes' pairs.
  bytes() {
    this.bytesMade ||= Buffer.concat([
      Buffer.from(Uint32Array.of(this.count, this.shapes).buffer),
      this.emission, this.lightBlock, this.fullFaces, this.faces, this.occludes,
    ]);
    return this.bytesMade;
  }
}

// "minecraft:oak_stairs[facing=east,half=bottom]" -> { facing: 'east', half: 'bottom' }.
function properties(name) {
  const open = name.indexOf('[');
  if (open < 0) return {};
  return Object.fromEntries(name.slice(open + 1, -1).split(',').map((pair) => pair.split('=')));
}

// The block states of the pinned Minecraft, read once from its block table,
// which the first start makes (minecraft-live.js blockTable; see_build runs
// only once setup is ready). A read that failed is tried again when next
// asked, as blockTable is.
let loaded = null;
function blockStates() {
  loaded ||= require('./minecraft-live').minecraftLive.blockTable()
    .then((file) => new BlockStates(fs.readFileSync(file, 'utf8')))
    .catch((error) => {
      loaded = null;
      throw error;
    });
  return loaded;
}

module.exports = { BlockStates, blockStates };
