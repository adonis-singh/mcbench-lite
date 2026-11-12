'use strict';

// Binary transfer of a voxel world across the build-process boundary. The
// server sends the whole pre-build world; the child returns the chunks it
// touched, the palette entries it interned, and its final chunk order. The
// return direction is untrusted (a program that escaped its JavaScript context
// controls every byte), so applyChanges re-normalizes palette entries against
// the block registry, bounds-checks every key and index, and recomputes all
// counts itself.

const { World, Chunk, MAX_BLOCKS, MAX_COORD, CHUNK_VOLUME } = require('./world');
const { validateWorldHeight } = require('./minecraft-dimension');

const SPARSE = 0;
const DENSE = 1;
const REMOVED = 2;
const DENSE_BYTES = CHUNK_VOLUME * 2;
const CHUNK_LIMIT = MAX_COORD >> 4;

class Reader {
  constructor(buffer) {
    this.buffer = buffer;
    this.offset = 0;
  }

  take(length) {
    if (this.offset + length > this.buffer.length) throw new RangeError('truncated');
    const start = this.offset;
    this.offset += length;
    return start;
  }

  u8() { return this.buffer.readUInt8(this.take(1)); }

  u16() { return this.buffer.readUInt16LE(this.take(2)); }

  u32() { return this.buffer.readUInt32LE(this.take(4)); }

  i32() { return this.buffer.readInt32LE(this.take(4)); }

  json() {
    const length = this.u32();
    const start = this.take(length);
    return JSON.parse(this.buffer.toString('utf8', start, start + length));
  }
}

function paletteJson(world, start) {
  return Buffer.from(JSON.stringify(world.palette.entries.slice(start).map((e) => [e.id, e.states])), 'utf8');
}

// [palette json][record count][records][order count][order keys]; the order
// section is present only in change sets. A record is [key][mode] followed,
// when `summaries` is set, by [count u16][local bbox 6 x u8, 255 = unknown],
// then the dense or sparse payload.
function encode(world, paletteStart, keys, order, summaries) {
  const palette = paletteJson(world, paletteStart);
  let size = 4 + palette.length + 4 + (order ? 4 + order.length * 4 : 0);
  for (const key of keys) {
    const chunk = world.chunks.get(key);
    size += 5 + (!chunk ? 0 : (summaries ? 8 : 0) + (chunk.dense ? DENSE_BYTES : 2 + chunk.sparse.size * 4));
  }
  const out = Buffer.allocUnsafe(size);
  let o = out.writeUInt32LE(palette.length, 0);
  o += palette.copy(out, o);
  o = out.writeUInt32LE(keys.length, o);
  for (const key of keys) {
    const chunk = world.chunks.get(key);
    o = out.writeInt32LE(key, o);
    if (!chunk) {
      o = out.writeUInt8(REMOVED, o);
      continue;
    }
    o = out.writeUInt8(chunk.dense ? DENSE : SPARSE, o);
    if (summaries) {
      o = out.writeUInt16LE(chunk.count, o);
      for (let i = 0; i < 6; i += 1) o = out.writeUInt8(chunk.bbox ? chunk.bbox[i] : 255, o);
    }
    if (chunk.dense) {
      o += Buffer.from(chunk.dense.buffer, chunk.dense.byteOffset, DENSE_BYTES).copy(out, o);
    } else {
      o = out.writeUInt16LE(chunk.sparse.size, o);
      for (const [index, value] of chunk.sparse) o = out.writeUInt16LE(value, out.writeUInt16LE(index, o));
    }
  }
  if (order) {
    o = out.writeUInt32LE(order.length, o);
    for (const key of order) o = out.writeInt32LE(key, o);
  }
  return out;
}

function readChunk(reader, remap) {
  const mode = reader.u8();
  if (mode === REMOVED) return null;
  const chunk = new Chunk();
  if (mode === DENSE) {
    const dense = new Uint16Array(CHUNK_VOLUME);
    const start = reader.take(DENSE_BYTES);
    new Uint8Array(dense.buffer).set(reader.buffer.subarray(start, start + DENSE_BYTES));
    let count = 0;
    for (let i = 0; i < CHUNK_VOLUME; i += 1) {
      const value = dense[i];
      if (value === 0) continue;
      if (value >= remap.length) throw new RangeError(`palette index ${value} out of range`);
      dense[i] = remap[value];
      count += 1;
    }
    chunk.sparse = null;
    chunk.dense = dense;
    chunk.count = count;
  } else if (mode === SPARSE) {
    for (let n = reader.u16(); n > 0; n -= 1) {
      const index = reader.u16();
      const value = reader.u16();
      if (index >= CHUNK_VOLUME || value === 0 || value >= remap.length) {
        throw new RangeError(`invalid block ${index}=${value}`);
      }
      chunk.sparse.set(index, remap[value]);
    }
    chunk.count = chunk.sparse.size;
  } else {
    throw new RangeError(`unknown chunk mode ${mode}`);
  }
  return chunk.count === 0 ? null : chunk;
}

function readKey(reader) {
  const key = reader.i32();
  const [cx, cy, cz] = World.coordsOf(key);
  if (World.keyOf(cx, cy, cz) !== key
    || Math.abs(cx) > CHUNK_LIMIT || Math.abs(cy) > CHUNK_LIMIT || Math.abs(cz) > CHUNK_LIMIT) {
    throw new RangeError(`chunk key ${key} out of range`);
  }
  return key;
}

function encodeWorld(world) {
  return encode(world, 1, [...world.chunks.keys()], null, true);
}

// Trusted direction: the parent's own world, with palette indices, counts,
// and cached bounds taken as sent rather than rescanned cell by cell.
function decodeWorld(buffer) {
  const reader = new Reader(buffer);
  const world = new World();
  for (const [id, states] of reader.json()) world.palette.intern(id, states);
  for (let n = reader.u32(); n > 0; n -= 1) {
    const key = reader.i32();
    const mode = reader.u8();
    const chunk = new Chunk();
    chunk.count = reader.u16();
    const bbox = [0, 0, 0, 0, 0, 0].map(() => reader.u8());
    chunk.bbox = bbox[0] === 255 ? null : bbox;
    if (mode === DENSE) {
      const start = reader.take(DENSE_BYTES);
      chunk.sparse = null;
      chunk.dense = new Uint16Array(CHUNK_VOLUME);
      new Uint8Array(chunk.dense.buffer).set(reader.buffer.subarray(start, start + DENSE_BYTES));
    } else {
      for (let i = reader.u16(); i > 0; i -= 1) chunk.sparse.set(reader.u16(), reader.u16());
    }
    world.chunks.set(key, chunk);
    world.blockCount += chunk.count;
  }
  return world;
}

function encodeChanges(world, paletteStart) {
  return encode(world, paletteStart, [...world.changedChunks], [...world.chunks.keys()], false);
}

// Untrusted direction. `world` is a fresh clone of the pre-build world whose
// palette had `paletteStart` entries when it was sent. On return the clone
// holds exactly what an in-process run would have left behind: untouched chunk
// objects shared with the original, new objects for touched keys, the same
// chunk iteration order, changed-chunk set, palette indices, and block count.
// Everything is checked, the build's height included, before anything is
// changed: on a throw the clone and its palette (shared with the original)
// are as they were.
function applyChanges(world, buffer, paletteStart, normalizeBlock) {
  const reader = new Reader(buffer);
  const blocks = reader.json().map(([id, states]) => {
    const block = normalizeBlock({ id, states }, 'palette entry');
    if (block.id === 'minecraft:air') throw new RangeError('air in palette');
    return block;
  });
  const remap = [...Array.from({ length: paletteStart }, (_, i) => i), ...world.palette.plan(blocks)];
  const changed = new Map();
  for (let n = reader.u32(); n > 0; n -= 1) {
    const key = readKey(reader);
    if (changed.has(key)) throw new RangeError(`chunk ${key} repeated`);
    changed.set(key, readChunk(reader, remap));
  }
  let expected = 0;
  for (const key of world.chunks.keys()) if (!changed.has(key)) expected += 1;
  for (const chunk of changed.values()) if (chunk) expected += 1;
  const chunks = new Map();
  let blockCount = 0;
  for (let n = reader.u32(); n > 0; n -= 1) {
    const key = reader.i32();
    const chunk = changed.has(key) ? changed.get(key) : world.chunks.get(key);
    if (!chunk || chunks.has(key)) throw new RangeError(`chunk order lists ${key} wrongly`);
    chunks.set(key, chunk);
    blockCount += chunk.count;
  }
  if (reader.offset !== reader.buffer.length) throw new RangeError('trailing bytes');
  if (chunks.size !== expected) throw new RangeError('chunk order is incomplete');
  if (blockCount > MAX_BLOCKS) throw new RangeError(`${blockCount} blocks exceed the limit`);
  // The child checked the height too, but its answer is untrusted.
  validateWorldHeight(new World({ chunks, palette: world.palette, blockCount }));
  for (const block of blocks) world.palette.intern(block.id, block.states);
  world.chunks = chunks;
  world.blockCount = blockCount;
  world.cachedBounds = null;
  for (const key of changed.keys()) {
    world.ownedChunks.add(key);
    world.changedChunks.add(key);
  }
}

module.exports = { encodeWorld, decodeWorld, encodeChanges, applyChanges };
