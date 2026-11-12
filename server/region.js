'use strict';

// Reads the chunks of a Minecraft dimension's region files (Anvil format,
// 1.18+ sections): the world the live viewer draws, and the worlds the
// parity scripts compare.

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

// NBT, with long arrays kept as their big-endian bytes: a section's block
// data is read straight from them (sectionStates).
function readNbt(buffer) {
  let at = 0;
  const u8 = () => buffer[at++];
  const i16 = () => { const v = buffer.readInt16BE(at); at += 2; return v; };
  const i32 = () => { const v = buffer.readInt32BE(at); at += 4; return v; };
  const text = () => { const n = buffer.readUInt16BE(at); at += 2; const v = buffer.toString('utf8', at, at + n); at += n; return v; };
  const payload = (type) => {
    switch (type) {
      case 1: return buffer.readInt8(at++);
      case 2: return i16();
      case 3: return i32();
      case 4: { const v = buffer.readBigInt64BE(at); at += 8; return v; }
      case 5: { const v = buffer.readFloatBE(at); at += 4; return v; }
      case 6: { const v = buffer.readDoubleBE(at); at += 8; return v; }
      case 7: { const n = i32(); const v = buffer.subarray(at, at + n); at += n; return v; }
      case 8: return text();
      case 9: { const element = u8(); const n = i32(); return Array.from({ length: n }, () => payload(element)); }
      case 10: {
        const out = {};
        for (let child = u8(); child !== 0; child = u8()) out[text()] = payload(child);
        return out;
      }
      case 11: { const n = i32(); const v = []; for (let i = 0; i < n; i += 1) v.push(i32()); return v; }
      case 12: { const n = i32(); const v = buffer.subarray(at, at + n * 8); at += n * 8; return v; }
      default: throw new Error(`Unknown NBT tag ${type}`);
    }
  };
  const type = u8();
  text();
  return payload(type);
}

// The chunk slots of one region file: per slot its stored (compressed)
// bytes, which name one exact version of the chunk.
function regionSlots(file) {
  const data = fs.readFileSync(file);
  const slots = [];
  if (data.length < 8192) return slots;
  for (let i = 0; i < 1024; i += 1) {
    const sector = data.readUIntBE(i * 4, 3);
    if (!sector) continue;
    const at = sector * 4096;
    const length = data.readInt32BE(at);
    slots.push({ index: i, stored: data.subarray(at + 4, at + 4 + length) });
  }
  return slots;
}

// A slot's chunk NBT.
function readChunk(stored) {
  const kind = stored[0];
  const raw = stored.subarray(1);
  return readNbt(kind === 2 ? zlib.inflateSync(raw) : kind === 1 ? zlib.gunzipSync(raw) : raw);
}

// Every saved chunk's NBT in a region folder.
function readChunks(folder) {
  if (!fs.existsSync(folder)) return [];
  return fs.readdirSync(folder).filter((entry) => entry.endsWith('.mca'))
    .flatMap((name) => regionSlots(path.join(folder, name)).map((slot) => readChunk(slot.stored)));
}

// A block state as Minecraft names it: "minecraft:id[key=value,...]",
// properties sorted.
function stateName(entry) {
  const properties = Object.entries(entry.Properties || {}).sort(([a], [b]) => (a < b ? -1 : 1));
  return properties.length ? `${entry.Name}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]` : entry.Name;
}

// A section's 4096 palette indices, in Minecraft's order (y, z, x), or null
// for a section of a single state. Entries never straddle two longs.
function sectionIndices(section) {
  const palette = section.block_states.palette;
  if (palette.length === 1) return null;
  const data = section.block_states.data;
  const bits = Math.max(4, Math.ceil(Math.log2(palette.length)));
  const perLong = Math.floor(64 / bits);
  const mask = (1 << bits) - 1;
  const indices = new Uint16Array(4096);
  for (let index = 0, long = 0; index < 4096; long += 1) {
    const high = data.readUInt32BE(long * 8);
    const low = data.readUInt32BE(long * 8 + 4);
    for (let k = 0, shift = 0; k < perLong && index < 4096; k += 1, index += 1, shift += bits) {
      const value = shift >= 32 ? high >>> (shift - 32)
        : shift + bits > 32 ? (low >>> shift) | (high << (32 - shift)) : low >>> shift;
      indices[index] = value & mask;
    }
  }
  return indices;
}

// Map of "x,y,z" -> state name for every non-air block.
function readBlockStates(folder) {
  const states = new Map();
  for (const chunk of readChunks(folder)) {
    for (const section of chunk.sections || []) {
      const palette = section.block_states?.palette || [];
      if (!palette.length) continue;
      const names = palette.map(stateName);
      const indices = sectionIndices(section);
      const origin = [chunk.xPos * 16, section.Y * 16, chunk.zPos * 16];
      for (let i = 0; i < 4096; i += 1) {
        const name = names[indices ? indices[i] : 0];
        if (name !== 'minecraft:air') states.set(`${origin[0] + (i & 15)},${origin[1] + (i >> 8)},${origin[2] + ((i >> 4) & 15)}`, name);
      }
    }
  }
  return states;
}

module.exports = { readNbt, regionSlots, readChunk, readChunks, stateName, sectionIndices, readBlockStates };
