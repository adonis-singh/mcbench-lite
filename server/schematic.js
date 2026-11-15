'use strict';

// Sponge Schematic v3 (.schem), which WorldEdit, FAWE and Litematica load.
// A chat's schematic is its build as the app shows it at tick 0: the world
// the engine holds once it has placed the build with Minecraft's own block
// updates (shot-world.js settle at 0 ticks, where the live world starts),
// so panes, fences, walls, dust and stairs are connected as Minecraft
// connects them and blocks Minecraft does not keep are gone. A paste sets
// the states as written, without updates, so they must be the final ones.
// Minecraft makes each block entity as its block is set.

const zlib = require('zlib');
const { blockSpec } = require('./live-placement');
const { MC_DATA_VERSION } = require('./assets');
const { settle } = require('./shot-world');

// Data holds one palette index per cell of the bounding box.
const MAX_VOLUME = 1 << 27;

const TAG = { short: 2, int: 3, byteArray: 7, string: 8, compound: 10, intArray: 11 };

function nbt(name, type, value) {
  const label = Buffer.from(name, 'utf8');
  const head = Buffer.alloc(3);
  head.writeUInt8(type, 0);
  head.writeUInt16BE(label.length, 1);
  return [head, label, payload(type, value)];
}

function payload(type, value) {
  if (type === TAG.short) { const b = Buffer.alloc(2); b.writeUInt16BE(value); return b; }
  if (type === TAG.int) { const b = Buffer.alloc(4); b.writeInt32BE(value); return b; }
  if (type === TAG.string) {
    const text = Buffer.from(value, 'utf8');
    const b = Buffer.alloc(2); b.writeUInt16BE(text.length);
    return Buffer.concat([b, text]);
  }
  if (type === TAG.byteArray) {
    const b = Buffer.alloc(4); b.writeInt32BE(value.length);
    return Buffer.concat([b, value]);
  }
  if (type === TAG.intArray) {
    const b = Buffer.alloc(4 + value.length * 4);
    b.writeInt32BE(value.length);
    value.forEach((v, i) => b.writeInt32BE(v, 4 + i * 4));
    return b;
  }
  // compound: [name, type, value] triples, then TAG_End
  return Buffer.concat([...value.flatMap(([n, t, v]) => nbt(n, t, v)), Buffer.alloc(1)]);
}

function writeSchematic(world) {
  const bounds = world.bounds();
  if (!bounds) throw new Error('The build is empty');
  const [width, height, length] = bounds.min.map((value, axis) => bounds.max[axis] - value + 1);
  const volume = width * height * length;
  if (volume > MAX_VOLUME) {
    throw new Error(`The build spans ${width}×${height}×${length} blocks, more than a schematic export holds (${MAX_VOLUME} cells)`);
  }

  const palette = new Map([['minecraft:air', 0]]);
  const local = new Map(); // world palette entry -> schematic palette index
  const cells = new Uint32Array(volume);
  const [ox, oy, oz] = bounds.min;
  world.forEach((x, y, z, entry) => {
    let index = local.get(entry);
    if (index === undefined) {
      const spec = blockSpec(entry);
      index = palette.get(spec);
      if (index === undefined) palette.set(spec, index = palette.size);
      local.set(entry, index);
    }
    cells[(x - ox) + (z - oz) * width + (y - oy) * width * length] = index;
  });

  // Indexes are unsigned LEB128 varints, x fastest, then z, then y.
  const data = Buffer.alloc(volume * (palette.size > 128 ? 3 : 1));
  let at = 0;
  for (let i = 0; i < volume; i += 1) {
    let value = cells[i];
    while (value >= 0x80) { data[at++] = (value & 0x7f) | 0x80; value >>>= 7; }
    data[at++] = value;
  }

  const root = payload(TAG.compound, [['Schematic', TAG.compound, [
    ['Version', TAG.int, 3],
    ['DataVersion', TAG.int, MC_DATA_VERSION],
    ['Width', TAG.short, width],
    ['Height', TAG.short, height],
    ['Length', TAG.short, length],
    ['Offset', TAG.intArray, [0, 0, 0]],
    ['Blocks', TAG.compound, [
      ['Palette', TAG.compound, [...palette].map(([spec, index]) => [spec, TAG.int, index])],
      ['Data', TAG.byteArray, data.subarray(0, at)],
    ]],
  ]]]);
  // The root is an unnamed compound.
  return zlib.gzipSync(Buffer.concat([Buffer.from([TAG.compound, 0, 0]), root]));
}

// The schematic of conversation `conversationId`'s `world` (its current
// world) at tick 0. The settle is kept with the world, so asking again
// writes the kept one.
async function buildSchematic(conversationId, world) {
  const { world: placed } = (await settle(conversationId, world, [0])).get(0);
  if (placed.size() === 0) throw Object.assign(new Error('Minecraft keeps none of the blocks of this build: there is nothing to export'), { statusCode: 400 });
  return writeSchematic(placed);
}

module.exports = { writeSchematic, buildSchematic };
