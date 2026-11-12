'use strict';

// A block's icon: the texture Minecraft breaks it into particles with, cut
// from the texture atlas as a PNG. Stairs show their planks and panes their
// glass; an animation shows its first frame; grass, foliage and water carry
// the plains biome's tint, as the game draws them there.

const zlib = require('zlib');
const { getAsset } = require('./assets');

const TINTS = [
  [/^birch_leaves$/, [0x80, 0xa7, 0x55]],
  [/^spruce_leaves$/, [0x61, 0x99, 0x61]],
  [/^(oak|jungle|acacia|dark_oak|mangrove)_leaves$|^vine$/, [0x77, 0xab, 0x2f]],
  [/^(short_grass|tall_grass|fern|large_fern|sugar_cane)$/, [0x91, 0xbd, 0x59]],
  [/^lily_pad$/, [0x20, 0x80, 0x30]],
  [/^(water|bubble_column)$/, [0x3f, 0x76, 0xe4]],
];

let assets = null;
const icons = new Map(); // block name -> PNG

function decodePng(buffer) {
  const width = buffer.readUInt32BE(16);
  const height = buffer.readUInt32BE(20);
  if (buffer[24] !== 8 || buffer[25] !== 6 || buffer[28] !== 0) throw new Error('The atlas must be a non-interlaced 8-bit RGBA PNG');
  const parts = [];
  for (let at = 8; at < buffer.length;) {
    const length = buffer.readUInt32BE(at);
    if (buffer.toString('latin1', at + 4, at + 8) === 'IDAT') parts.push(buffer.subarray(at + 8, at + 8 + length));
    at += length + 12;
  }
  const raw = zlib.inflateSync(Buffer.concat(parts));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const out = y * stride;
    for (let x = 0; x < stride; x += 1) {
      const left = x >= 4 ? pixels[out + x - 4] : 0;
      const up = y > 0 ? pixels[out - stride + x] : 0;
      const corner = x >= 4 && y > 0 ? pixels[out - stride + x - 4] : 0;
      let value = line[x];
      if (filter === 1) value += left;
      else if (filter === 2) value += up;
      else if (filter === 3) value += (left + up) >> 1;
      else if (filter === 4) {
        const estimate = left + up - corner;
        const toLeft = Math.abs(estimate - left);
        const toUp = Math.abs(estimate - up);
        const toCorner = Math.abs(estimate - corner);
        value += toLeft <= toUp && toLeft <= toCorner ? left : toUp <= toCorner ? up : corner;
      }
      pixels[out + x] = value & 255;
    }
  }
  return { width, pixels };
}

function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, 'latin1');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(zlib.crc32(Buffer.concat([head.subarray(4), data])), 0);
  return Buffer.concat([head, data, crc]);
}

function encodePng(size, pixels) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header.set([8, 6, 0, 0, 0], 8);
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y += 1) pixels.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', zlib.deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

async function loadAssets() {
  if (assets) return assets;
  const [definitions, models, atlasMap, atlas] = await Promise.all([
    getAsset('block_definitions.json'), getAsset('models.json'), getAsset('atlas_map.json'), getAsset('atlas.png'),
  ]);
  assets = { definitions: JSON.parse(definitions), models: JSON.parse(models), atlasMap: JSON.parse(atlasMap), atlas: decodePng(atlas) };
  return assets;
}

// The model's particle texture, following its parents and #references.
function particleTexture(name, { definitions, models }) {
  const definition = definitions[name];
  const first = definition.variants ? Object.values(definition.variants)[0] : definition.multipart[0].apply;
  const textures = {};
  // builtin/ parents have no model file.
  for (let model = (Array.isArray(first) ? first[0] : first).model; models[model?.replace(/^minecraft:/, '')];) {
    const entry = models[model.replace(/^minecraft:/, '')];
    for (const [key, value] of Object.entries(entry.textures || {})) if (!(key in textures)) textures[key] = value;
    model = entry.parent;
  }
  let texture = textures.particle;
  while (texture.startsWith('#')) texture = textures[texture.slice(1)];
  return texture.replace(/^minecraft:/, '');
}

// The PNG for a block name such as "glass", or null for a name that is not a block.
async function blockIcon(name) {
  if (icons.has(name)) return icons.get(name);
  const loaded = await loadAssets();
  if (!Object.hasOwn(loaded.definitions, name)) return null;
  const [x, y, size] = loaded.atlasMap[particleTexture(name, loaded)];
  const { width, pixels } = loaded.atlas;
  const tint = TINTS.find(([pattern]) => pattern.test(name))?.[1];
  const icon = Buffer.alloc(size * size * 4);
  for (let row = 0; row < size; row += 1) {
    pixels.copy(icon, row * size * 4, ((y + row) * width + x) * 4, ((y + row) * width + x + size) * 4);
  }
  if (tint) {
    for (let at = 0; at < icon.length; at += 4) {
      for (let channel = 0; channel < 3; channel += 1) icon[at + channel] = Math.round(icon[at + channel] * tint[channel] / 255);
    }
  }
  const png = encodePng(size, icon);
  icons.set(name, png);
  return png;
}

module.exports = { blockIcon, _internal: { decodePng, encodePng, particleTexture } };
