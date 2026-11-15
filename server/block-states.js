'use strict';

const fs = require('fs');

// What the pinned Minecraft says of each of its block states, read from the
// block table (McbenchBlockTable.java) and the render layers
// (McbenchClient.java), by state id (the table's order, which is
// Minecraft's registry order, air first): the light it emits, the light it
// blocks, its full faces, the face shapes Paper's light engine tests light
// through, with whether each pair of face shapes occludes; its fluid,
// whether it is drawn as a model, which neighbours it hides faces from
// (skipRendering), the face shapes Block.shouldRenderFace culls by, with
// whether each pair hides a face, and whether its model and its fluid are
// drawn blended; and each block's name, as Minecraft names it.
// see_build's page lights, culls and draws with these (`bytes`, read by
// web/viewer.js); the world route hides blocks with them and tells the page
// each palette entry's state id (`ofPalette`).
class BlockStates {
  // `table`: the block table's lines; `layers`: the render layers' lines.
  constructor(table, layers) {
    const lines = table.split('\n').filter(Boolean);
    const [, rows, columns, ...skipRows] = lines.pop().split('\t');
    const [, shapes, pairs] = lines.pop().split('\t');
    const [, cullShapes, hides] = lines.pop().split('\t');
    this.shapes = Number(shapes);
    this.cullShapes = Number(cullShapes);
    // A face shape's number is kept in a byte.
    if (this.shapes > 256 || this.cullShapes > 256) throw new Error(`The block table has ${this.shapes} face shapes and ${this.cullShapes} cull shapes, more than a byte numbers`);
    this.occludes = Uint8Array.from(pairs, (bit) => (bit === '1' ? 1 : 0));
    this.hides = Uint8Array.from(hides, (bit) => (bit === '1' ? 1 : 0));
    this.defaults = new Map(); // block id -> its default state's properties
    this.names = new Map(); // block id -> its name ("Block of Iron")
    this.ids = new Map(); // state name, properties sorted -> state id
    const states = [];
    for (const line of lines) {
      const fields = line.split('\t');
      if (fields[0] === 'default') {
        this.defaults.set(fields[1], properties(fields[2]));
        this.names.set(fields[1], fields[3]);
      } else states.push(fields);
    }
    this.count = states.length;
    // A state id, and a class of skipRendering, is kept in 16 bits.
    if (this.count > 0x10000 || Number(rows) > 0x10000 || Number(columns) > 0x10000) throw new Error('The block table has more states or skipRendering classes than 16 bits number');
    this.emission = new Uint8Array(this.count);
    this.lightBlock = new Uint8Array(this.count);
    this.fullFaces = new Uint8Array(this.count);
    this.faces = new Uint8Array(this.count * 6);
    this.cull = new Uint8Array(this.count * 6);
    this.fluid = new Uint16Array(this.count);
    // By state: 1 drawn as a model, 2 its model drawn blended, 4 its fluid
    // drawn blended, 8 it darkens the corners of the faces beside it.
    this.draw = new Uint8Array(this.count);
    this.skipRow = new Uint16Array(this.count);
    this.skipColumn = new Uint16Array(this.count);
    states.forEach(([name, emission, lightBlock, full, faces, fluid, model, skip, cull, shades], id) => {
      this.ids.set(name, id);
      this.emission[id] = Number(emission);
      this.lightBlock[id] = Number(lightBlock);
      this.fullFaces[id] = Number(full);
      faces.split(',').forEach((shape, direction) => { this.faces[id * 6 + direction] = Number(shape); });
      cull.split(',').forEach((shape, direction) => { this.cull[id * 6 + direction] = Number(shape); });
      this.fluid[id] = Number(fluid);
      this.draw[id] = Number(model) | (Number(shades) << 3);
      const [row, column] = skip.split(',');
      this.skipRow[id] = Number(row);
      this.skipColumn[id] = Number(column);
    });
    // By skipRendering row class: the first column class it has a mask for,
    // how many it has, and where they start in `skipMasks` (bits over
    // Minecraft's directions: down, up, north, south, west, east).
    this.skipFirst = new Uint16Array(skipRows.length);
    this.skipLength = new Uint16Array(skipRows.length);
    this.skipStart = new Uint32Array(skipRows.length);
    const masks = [];
    skipRows.forEach((field, row) => {
      const [first, ...rowMasks] = field.split(',').map(Number);
      this.skipFirst[row] = first;
      this.skipLength[row] = rowMasks.length;
      this.skipStart[row] = masks.length;
      masks.push(...rowMasks);
    });
    this.skipMasks = Uint8Array.from(masks);

    // The render layers, by the table's states, line for line.
    const layerLines = layers.split('\n').filter(Boolean);
    const blended = [];
    while (layerLines[0].startsWith('layer\t')) blended.push(layerLines.shift().split('\t')[2] === '1');
    if (layerLines.length !== this.count) throw new Error(`The render layers have ${layerLines.length} states, the block table ${this.count}`);
    layerLines.forEach((line, id) => {
      const [name, own, fluid] = line.split('\t');
      if (states[id][0] !== name) throw new Error(`The render layers name state ${id} ${name}, the block table ${states[id][0]}`);
      if (blended[Number(own)]) this.draw[id] |= 2;
      if (blended[Number(fluid)]) this.draw[id] |= 4;
    });
    // The fluids' states by name, for the page to draw them.
    this.fluidNames = Object.fromEntries([...new Set(this.fluid)].filter(Boolean).map((id) => [id, states[id][0]]));
    this.palettes = new WeakMap();
  }

  // Block.shouldRenderFace: whether state `a` hides its face toward state
  // `b`, its neighbour to Minecraft's direction `direction` (down, up,
  // north, south, west, east by Direction.ordinal()).
  hidesFace(a, b, direction) {
    return this.hides[this.cull[a * 6 + direction] * this.cullShapes + this.cull[b * 6 + (direction ^ 1)]] === 1 || this.skips(a, b, direction);
  }

  // BlockState.skipRendering: whether state `a` hides its face toward
  // state `b`, its neighbour to Minecraft's direction `direction`.
  skips(a, b, direction) {
    const row = this.skipRow[a];
    const at = this.skipColumn[b] - this.skipFirst[row];
    return at >= 0 && at < this.skipLength[row] && ((this.skipMasks[this.skipStart[row] + at] >> direction) & 1) === 1;
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

  // What the page reads (web/viewer.js loadResources), each part starting
  // on four bytes: u32 states, face shapes, cull shapes, skipRendering row
  // classes, mask bytes and fluid names' bytes; by state u16 fluid,
  // skipRendering row and column; by row class u32 start, u16 first and
  // length; then by state its emission, light block, full faces and draw
  // bits, its six face shapes and its six cull shapes, the face shapes'
  // pairs, the cull shapes' pairs, the skipRendering masks, and the fluids'
  // names (JSON, state id -> name).
  bytes() {
    if (this.bytesMade) return this.bytesMade;
    const names = Buffer.from(JSON.stringify(this.fluidNames));
    const parts = [
      Uint32Array.of(this.count, this.shapes, this.cullShapes, this.skipFirst.length, this.skipMasks.length, names.length),
      this.fluid, this.skipRow, this.skipColumn,
      this.skipStart, this.skipFirst, this.skipLength,
      this.emission, this.lightBlock, this.fullFaces, this.draw, this.faces, this.cull, this.occludes, this.hides, this.skipMasks, names,
    ].map((part) => Buffer.from(part.buffer, part.byteOffset, part.byteLength));
    this.bytesMade = Buffer.concat(parts.flatMap((part) => [part, Buffer.alloc((4 - (part.length % 4)) % 4)]));
    return this.bytesMade;
  }
}

// "minecraft:oak_stairs[facing=east,half=bottom]" -> { facing: 'east', half: 'bottom' }.
function properties(name) {
  const open = name.indexOf('[');
  if (open < 0) return {};
  return Object.fromEntries(name.slice(open + 1, -1).split(',').map((pair) => pair.split('=')));
}

// The block states of the pinned Minecraft, read once from its block table
// and render layers, which the first start makes (minecraft-live.js
// blockTable and renderLayers; see_build runs only once setup is ready). A
// read that failed is tried again when next asked, as those are.
let loaded = null;
function blockStates() {
  const { minecraftLive } = require('./minecraft-live');
  loaded ||= Promise.all([minecraftLive.blockTable(), minecraftLive.renderLayers()])
    .then(([table, layers]) => new BlockStates(fs.readFileSync(table, 'utf8'), fs.readFileSync(layers, 'utf8')))
    .catch((error) => {
      loaded = null;
      throw error;
    });
  return loaded;
}

module.exports = { BlockStates, blockStates };
