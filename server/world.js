'use strict';

// Chunked sparse voxel world. Blocks are stored as integer palette references
// inside 16^3 chunks; cloning copies only the chunk index and each changed
// chunk is copied on first write. Every per-block path is a typed-array loop:
// chunk keys are packed integers, bounds come from cached per-chunk boxes,
// and iteration is callback based. Build cost is proportional to the edit,
// never to the world.

const MAX_COORD = 4096;
const MAX_BLOCKS = 3_000_000;
const CHUNK_SIZE = 16;
const CHUNK_VOLUME = CHUNK_SIZE ** 3;
const DENSE_THRESHOLD = 512;
const MAX_PALETTE = 65535; // dense chunks are Uint16
const KEY_OFFSET = 512; // chunk coords lie within +/-256; packed as 3 x 10 bits

function keyOf(cx, cy, cz) {
  return ((cx + KEY_OFFSET) << 20) | ((cy + KEY_OFFSET) << 10) | (cz + KEY_OFFSET);
}

function coordsOf(key) {
  return [(key >>> 20) - KEY_OFFSET, ((key >>> 10) & 1023) - KEY_OFFSET, (key & 1023) - KEY_OFFSET];
}

function stateSignature(id, states) {
  const keys = Object.keys(states || {}).sort();
  return `${id}|${keys.map((key) => `${key}=${states[key]}`).join(',')}`;
}

// Process-unique chunk version stamps: equal (key, version) means equal
// content for the lifetime of this process, across clones and branches.
let stamp = 0;

// The boundary layers of chunks (World.shownVersion), each as the cells of
// it with six full faces, 256 bits, numbered by content: the layer of none
// is 0. Few layers are told apart (all, none, and the edges of shapes built).
const LAYERS = new Map();
function layerNumber(bits) {
  const key = String.fromCharCode(...bits);
  let number = LAYERS.get(key);
  if (number === undefined) LAYERS.set(key, number = LAYERS.size);
  return number;
}
const EMPTY_LAYER = layerNumber(new Uint8Array(32));
// A chunk's six boundary layers (x = 0, x = 15, y = 0, y = 15, z = 0,
// z = 15), by `occluding` (block-states.js ofPalette), kept for the chunk's
// version.
const heldLayers = new WeakMap();
function boundaryLayers(chunk, occluding) {
  const held = heldLayers.get(chunk);
  if (held && held.version === chunk.version && held.occluding === occluding) return held.layers;
  const dense = chunk.view();
  const bits = Array.from({ length: 6 }, () => new Uint8Array(32));
  for (let a = 0; a < 16; a += 1) {
    for (let b = 0; b < 16; b += 1) {
      const byte = (a << 1) | (b >> 3);
      const bit = 1 << (b & 7);
      if (occluding[dense[(a << 8) | (b << 4)]]) bits[0][byte] |= bit;
      if (occluding[dense[(a << 8) | (b << 4) | 15]]) bits[1][byte] |= bit;
      if (occluding[dense[(a << 4) | b]]) bits[2][byte] |= bit;
      if (occluding[dense[(15 << 8) | (a << 4) | b]]) bits[3][byte] |= bit;
      if (occluding[dense[(a << 8) | b]]) bits[4][byte] |= bit;
      if (occluding[dense[(a << 8) | (15 << 4) | b]]) bits[5][byte] |= bit;
    }
  }
  const layers = bits.map(layerNumber);
  heldLayers.set(chunk, { version: chunk.version, occluding, layers });
  return layers;
}

class PaletteStore {
  constructor(entries = []) {
    this.entries = [null]; // zero is air
    this.bySignature = new Map();
    for (const entry of entries) this.intern(entry.id, entry.states || {});
  }

  intern(id, states = {}) {
    const signature = stateSignature(id, states);
    let index = this.bySignature.get(signature);
    if (index !== undefined) return index;
    index = this.entries.length;
    if (index > MAX_PALETTE) throw new Error(`Build uses more than ${MAX_PALETTE} distinct block states`);
    this.entries.push(Object.freeze({ id, states: Object.freeze({ ...states }) }));
    this.bySignature.set(signature, index);
    return index;
  }

  // The indices interning `blocks` ({id, states}) in order would give,
  // without changing the store.
  plan(blocks) {
    const added = new Map(); // signature -> index
    return blocks.map(({ id, states = {} }) => {
      const signature = stateSignature(id, states);
      let index = this.bySignature.get(signature) ?? added.get(signature);
      if (index !== undefined) return index;
      index = this.entries.length + added.size;
      if (index > MAX_PALETTE) throw new Error(`Build uses more than ${MAX_PALETTE} distinct block states`);
      added.set(signature, index);
      return index;
    });
  }

  get(index) {
    return this.entries[index] || null;
  }
}

class Chunk {
  constructor() {
    this.sparse = new Map();
    this.dense = null;
    this.count = 0;
    this.version = ++stamp;
    this.persistedHash = null;
    this.contentHash = null; // the snapshot key's hash of these blocks
    this.bbox = null; // local [minX, minY, minZ, maxX, maxY, maxZ]
  }

  clone() {
    const chunk = new Chunk();
    chunk.sparse = this.dense ? null : new Map(this.sparse);
    chunk.dense = this.dense ? this.dense.slice() : null;
    chunk.count = this.count;
    chunk.version = this.version;
    chunk.persistedHash = this.persistedHash;
    chunk.contentHash = this.contentHash;
    chunk.bbox = this.bbox;
    return chunk;
  }

  touch() {
    this.version = ++stamp;
    this.persistedHash = null;
    this.contentHash = null;
    this.bbox = null;
  }

  get(index) {
    return this.dense ? this.dense[index] : (this.sparse.get(index) || 0);
  }

  ensureDense() {
    if (!this.dense) {
      const dense = new Uint16Array(CHUNK_VOLUME);
      for (const [index, paletteIndex] of this.sparse) dense[index] = paletteIndex;
      this.dense = dense;
      this.sparse = null;
    }
    return this.dense;
  }

  // Dense read view; sparse chunks get a scratch copy.
  view() {
    if (this.dense) return this.dense;
    const dense = new Uint16Array(CHUNK_VOLUME);
    for (const [index, paletteIndex] of this.sparse) dense[index] = paletteIndex;
    return dense;
  }

  set(index, value) {
    const previous = this.get(index);
    if (previous === value) return previous;
    if (this.dense) {
      this.dense[index] = value;
    } else if (value === 0) {
      this.sparse.delete(index);
    } else {
      this.sparse.set(index, value);
      if (this.sparse.size > DENSE_THRESHOLD) this.ensureDense();
    }
    if (previous === 0) this.count += 1;
    else if (value === 0) this.count -= 1;
    this.touch();
    return previous;
  }

  forEach(fn) {
    if (this.dense) {
      const dense = this.dense;
      for (let index = 0; index < CHUNK_VOLUME; index += 1) {
        if (dense[index] !== 0) fn(index, dense[index]);
      }
    } else {
      for (const [index, paletteIndex] of this.sparse) fn(index, paletteIndex);
    }
  }

  // [index, paletteIndex] pairs in index order (deterministic persistence).
  pairs() {
    const pairs = [];
    if (this.dense) {
      this.forEach((index, paletteIndex) => pairs.push([index, paletteIndex]));
    } else {
      for (const index of [...this.sparse.keys()].sort((a, b) => a - b)) pairs.push([index, this.sparse.get(index)]);
    }
    return pairs;
  }

  bounds() {
    if (this.bbox) return this.bbox;
    if (this.count === 0) return null;
    let minX = CHUNK_SIZE; let minY = CHUNK_SIZE; let minZ = CHUNK_SIZE;
    let maxX = -1; let maxY = -1; let maxZ = -1;
    this.forEach((index) => {
      const lx = index & 15;
      const lz = (index >> 4) & 15;
      const ly = index >> 8;
      if (lx < minX) minX = lx; if (lx > maxX) maxX = lx;
      if (ly < minY) minY = ly; if (ly > maxY) maxY = ly;
      if (lz < minZ) minZ = lz; if (lz > maxZ) maxZ = lz;
    });
    this.bbox = [minX, minY, minZ, maxX, maxY, maxZ];
    return this.bbox;
  }
}

class World {
  constructor({ chunks, palette, blockCount = 0, revision = 0, base = null, settleTicks = null } = {}) {
    this.chunks = chunks || new Map(); // packed int key -> Chunk
    this.palette = palette || new PaletteStore();
    this.blockCount = blockCount;
    this.revision = revision;
    this.base = base;
    // The ticks Minecraft settled this world with for the build that made it
    // (shot-world.js currentTicks), or null for the default.
    this.settleTicks = settleTicks;
    this.ownedChunks = new Set();
    this.changedChunks = new Set();
    this.cachedBounds = null;
  }

  static keyOf(cx, cy, cz) { return keyOf(cx, cy, cz); }

  static coordsOf(key) { return coordsOf(key); }

  // A copy sharing this world's chunks until either writes one. Chunks this
  // world wrote since its last finalize it would keep writing in place, under
  // the copy, so it must be finalized first.
  clone() {
    if (this.ownedChunks.size) throw new Error('A world with writes since its last finalize cannot be cloned: finalize it first');
    return new World({
      chunks: new Map(this.chunks),
      palette: this.palette,
      blockCount: this.blockCount,
      revision: this.revision,
      base: this,
      settleTicks: this.settleTicks,
    });
  }

  checkCoord(x, y, z) {
    if (!Number.isInteger(x) || !Number.isInteger(y) || !Number.isInteger(z)
      || x < -MAX_COORD || x > MAX_COORD || y < -MAX_COORD || y > MAX_COORD || z < -MAX_COORD || z > MAX_COORD) {
      throw new Error(`Position [${x}, ${y}, ${z}] is out of range (each axis must be an integer within +/-${MAX_COORD})`);
    }
  }

  paletteIndexAt(x, y, z) {
    const chunk = this.chunks.get(keyOf(x >> 4, y >> 4, z >> 4));
    return chunk ? chunk.get(((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) : 0;
  }

  writableChunk(key) {
    let chunk = this.chunks.get(key);
    if (!chunk) {
      chunk = new Chunk();
      this.chunks.set(key, chunk);
      this.ownedChunks.add(key);
      return chunk;
    }
    // A chunk this world did not create since its last finalize may be
    // shared (with the world it was cloned from, a clone of it, or another
    // world loaded from the same stored chunk), so it is copied first.
    if (!this.ownedChunks.has(key)) {
      chunk = chunk.clone();
      this.chunks.set(key, chunk);
      this.ownedChunks.add(key);
    }
    return chunk;
  }

  set(x, y, z, id, states = {}) {
    this.checkCoord(x, y, z);
    const key = keyOf(x >> 4, y >> 4, z >> 4);
    const index = ((y & 15) << 8) | ((z & 15) << 4) | (x & 15);
    const existing = this.chunks.get(key);
    const current = existing ? existing.get(index) : 0;
    // Refused before anything changes, the palette included.
    if (current === 0 && id !== 'minecraft:air' && this.blockCount >= MAX_BLOCKS) this.refuse(1);
    const next = id === 'minecraft:air' ? 0 : this.palette.intern(id, states);
    if (current === next) return;
    const chunk = this.writableChunk(key);
    chunk.set(index, next);
    if (current === 0) this.blockCount += 1;
    else if (next === 0) this.blockCount -= 1;
    if (chunk.count === 0) this.chunks.delete(key);
    this.changedChunks.add(key);
    this.cachedBounds = null;
  }

  refuse(added) {
    throw new Error(`Build exceeds the ${MAX_BLOCKS} block limit: the world has ${this.blockCount} blocks and this call would add ${added}`);
  }

  // How many blocks a fill of the box would add: its air cells.
  airIn(min, max) {
    let air = 0;
    for (let cx = min[0] >> 4; cx <= max[0] >> 4; cx += 1) {
      const fromX = Math.max(min[0] - cx * CHUNK_SIZE, 0);
      const toX = Math.min(max[0] - cx * CHUNK_SIZE, CHUNK_SIZE - 1);
      for (let cy = min[1] >> 4; cy <= max[1] >> 4; cy += 1) {
        const fromY = Math.max(min[1] - cy * CHUNK_SIZE, 0);
        const toY = Math.min(max[1] - cy * CHUNK_SIZE, CHUNK_SIZE - 1);
        for (let cz = min[2] >> 4; cz <= max[2] >> 4; cz += 1) {
          const fromZ = Math.max(min[2] - cz * CHUNK_SIZE, 0);
          const toZ = Math.min(max[2] - cz * CHUNK_SIZE, CHUNK_SIZE - 1);
          const volume = (toX - fromX + 1) * (toY - fromY + 1) * (toZ - fromZ + 1);
          const chunk = this.chunks.get(keyOf(cx, cy, cz));
          if (!chunk) {
            air += volume;
          } else if (volume === CHUNK_VOLUME) {
            air += CHUNK_VOLUME - chunk.count;
          } else {
            for (let ly = fromY; ly <= toY; ly += 1) {
              for (let lz = fromZ; lz <= toZ; lz += 1) {
                let index = (ly << 8) | (lz << 4) | fromX;
                for (let lx = fromX; lx <= toX; lx += 1, index += 1) {
                  if (chunk.get(index) === 0) air += 1;
                }
              }
            }
          }
        }
      }
    }
    return air;
  }

  fill(min, max, id, states = {}) {
    this.fillBoxes([[min, max]], id, states);
  }

  // Sets the boxes in `clear` to air and fills the boxes in `parts` with one
  // block; no two boxes overlap. The block limit applies to what the call
  // leaves, and a call it refuses writes nothing.
  fillBoxes(parts, id, states = {}, clear = []) {
    for (const [min, max] of [...parts, ...clear]) {
      this.checkCoord(min[0], min[1], min[2]);
      this.checkCoord(max[0], max[1], max[2]);
    }
    const volume = ([min, max]) => (max[0] - min[0] + 1) * (max[1] - min[1] + 1) * (max[2] - min[2] + 1);
    const room = MAX_BLOCKS - this.blockCount;
    // Counted only when the boxes' volume alone is past the room left.
    if (id !== 'minecraft:air' && parts.reduce((n, box) => n + volume(box), 0) > room) {
      let added = 0;
      for (const box of parts) added += this.airIn(box[0], box[1]);
      for (const box of clear) added -= volume(box) - this.airIn(box[0], box[1]);
      if (added > room) this.refuse(added);
    }
    for (const [min, max] of clear) this.writeBox(min, max, 0);
    const next = id === 'minecraft:air' ? 0 : this.palette.intern(id, states);
    for (const [min, max] of parts) this.writeBox(min, max, next);
  }

  writeBox(min, max, next) {
    for (let cx = min[0] >> 4; cx <= max[0] >> 4; cx += 1) {
      const fromX = Math.max(min[0] - cx * CHUNK_SIZE, 0);
      const toX = Math.min(max[0] - cx * CHUNK_SIZE, CHUNK_SIZE - 1);
      for (let cy = min[1] >> 4; cy <= max[1] >> 4; cy += 1) {
        const fromY = Math.max(min[1] - cy * CHUNK_SIZE, 0);
        const toY = Math.min(max[1] - cy * CHUNK_SIZE, CHUNK_SIZE - 1);
        for (let cz = min[2] >> 4; cz <= max[2] >> 4; cz += 1) {
          const fromZ = Math.max(min[2] - cz * CHUNK_SIZE, 0);
          const toZ = Math.min(max[2] - cz * CHUNK_SIZE, CHUNK_SIZE - 1);
          const key = keyOf(cx, cy, cz);
          if (next === 0 && !this.chunks.has(key)) continue;
          const chunk = this.writableChunk(key);
          const volume = (toX - fromX + 1) * (toY - fromY + 1) * (toZ - fromZ + 1);
          const dense = chunk.dense || volume > DENSE_THRESHOLD ? chunk.ensureDense() : null;
          let added = 0;
          let removed = 0;
          let mutated = false;
          for (let ly = fromY; ly <= toY; ly += 1) {
            for (let lz = fromZ; lz <= toZ; lz += 1) {
              let index = (ly << 8) | (lz << 4) | fromX;
              for (let lx = fromX; lx <= toX; lx += 1, index += 1) {
                const current = dense ? dense[index] : chunk.get(index);
                if (current === next) continue;
                if (current === 0) added += 1;
                else if (next === 0) removed += 1;
                mutated = true;
                if (dense) dense[index] = next;
                else chunk.set(index, next);
              }
            }
          }
          if (!mutated) continue;
          if (dense) {
            chunk.count += added - removed;
            chunk.touch();
          }
          this.blockCount += added - removed;
          if (chunk.count === 0) this.chunks.delete(key);
          this.changedChunks.add(key);
        }
      }
    }
    this.cachedBounds = null;
  }

  get(x, y, z) {
    return this.palette.get(this.paletteIndexAt(x, y, z));
  }

  // Resolve replacement states once per palette entry, then rewrite indexes
  // in place. Each chunk is copied on its first actual change.
  replace(id, replacement, box = null) {
    const entries = this.palette.entries;
    const matches = entries.map((entry) => entry?.id === id);
    if (!matches.includes(true)) return 0;
    const remap = new Int32Array(entries.length).fill(-1);
    let count = 0;
    for (const [key, original] of this.chunks) {
      const [cx, cy, cz] = coordsOf(key);
      const ox = cx * CHUNK_SIZE; const oy = cy * CHUNK_SIZE; const oz = cz * CHUNK_SIZE;
      if (box && (ox > box.max[0] || ox + 15 < box.min[0]
        || oy > box.max[1] || oy + 15 < box.min[1]
        || oz > box.max[2] || oz + 15 < box.min[2])) continue;
      let chunk = null;
      let removed = 0;
      original.forEach((index, previous) => {
        if (!matches[previous]) return;
        if (box) {
          const x = ox + (index & 15); const y = oy + (index >> 8); const z = oz + ((index >> 4) & 15);
          if (x < box.min[0] || x > box.max[0] || y < box.min[1] || y > box.max[1]
            || z < box.min[2] || z > box.max[2]) return;
        }
        count += 1;
        let next = remap[previous];
        if (next === -1) {
          const entry = replacement(entries[previous]);
          next = entry.id === 'minecraft:air' ? 0 : this.palette.intern(entry.id, entry.states);
          remap[previous] = next;
        }
        if (next === previous) return;
        if (!chunk) chunk = this.writableChunk(key);
        if (chunk.dense) chunk.dense[index] = next;
        else if (next === 0) chunk.sparse.delete(index);
        else chunk.sparse.set(index, next);
        if (next === 0) removed += 1;
      });
      if (!chunk) continue;
      chunk.count -= removed;
      chunk.touch();
      this.blockCount -= removed;
      if (chunk.count === 0) this.chunks.delete(key);
      this.changedChunks.add(key);
      this.cachedBounds = null;
    }
    return count;
  }

  has(x, y, z) {
    return this.paletteIndexAt(x, y, z) !== 0;
  }

  size() {
    return this.blockCount;
  }

  clear() {
    if (this.blockCount === 0) return;
    for (const key of this.chunks.keys()) this.changedChunks.add(key);
    this.chunks = new Map();
    this.ownedChunks = new Set();
    this.blockCount = 0;
    this.cachedBounds = null;
  }

  reference() {
    return new World({
      chunks: new Map(this.chunks),
      palette: this.palette,
      blockCount: this.blockCount,
      revision: this.revision,
      settleTicks: this.settleTicks,
    });
  }

  // fn(x, y, z, entry) for every block of one chunk.
  forEachInChunk(key, fn) {
    const chunk = this.chunks.get(key);
    if (!chunk) return;
    const [cx, cy, cz] = coordsOf(key);
    const ox = cx * CHUNK_SIZE; const oy = cy * CHUNK_SIZE; const oz = cz * CHUNK_SIZE;
    const { entries } = this.palette;
    chunk.forEach((index, paletteIndex) => {
      fn(ox + (index & 15), oy + (index >> 8), oz + ((index >> 4) & 15), entries[paletteIndex]);
    });
  }

  // fn(x, y, z, entry) for every block, optionally only inside box {min, max}.
  forEach(fn, box = null) {
    if (!box) {
      for (const key of this.chunks.keys()) this.forEachInChunk(key, fn);
      return;
    }
    const [minX, minY, minZ] = box.min;
    const [maxX, maxY, maxZ] = box.max;
    for (let cx = minX >> 4; cx <= maxX >> 4; cx += 1) {
      for (let cy = minY >> 4; cy <= maxY >> 4; cy += 1) {
        for (let cz = minZ >> 4; cz <= maxZ >> 4; cz += 1) {
          this.forEachInChunk(keyOf(cx, cy, cz), (x, y, z, entry) => {
            if (x >= minX && x <= maxX && y >= minY && y <= maxY && z >= minZ && z <= maxZ) fn(x, y, z, entry);
          });
        }
      }
    }
  }

  bounds() {
    if (this.cachedBounds) return this.cachedBounds;
    if (this.blockCount === 0) return null;
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (const [key, chunk] of this.chunks) {
      const box = chunk.bounds();
      const [cx, cy, cz] = coordsOf(key);
      const ox = cx * CHUNK_SIZE; const oy = cy * CHUNK_SIZE; const oz = cz * CHUNK_SIZE;
      if (ox + box[0] < min[0]) min[0] = ox + box[0];
      if (oy + box[1] < min[1]) min[1] = oy + box[1];
      if (oz + box[2] < min[2]) min[2] = oz + box[2];
      if (ox + box[3] > max[0]) max[0] = ox + box[3];
      if (oy + box[4] > max[1]) max[1] = oy + box[4];
      if (oz + box[5] > max[2]) max[2] = oz + box[5];
    }
    this.cachedBounds = { min, max };
    return this.cachedBounds;
  }

  renderLod() {
    const bounds = this.bounds();
    if (!bounds || this.blockCount < 500_000) return null;
    const dimensions = bounds.min.map((value, index) => bounds.max[index] - value + 1);
    const axis = dimensions.indexOf(Math.min(...dimensions));
    if (dimensions[axis] > CHUNK_SIZE || Math.max(...dimensions) <= 1024) return null;
    return { axis, scale: 4 };
  }

  // A chunk as see_build's page draws it: its blocks with their palette
  // entries, each entry with its Minecraft state id, less the blocks hidden
  // on every side (a block whose six neighbours and itself have six full
  // faces: such a state blocks all light, so the block is dark, and every
  // face of it and every face turned to it is hidden), and grouped into
  // `lod` proxies where a render LOD is set. `states`: the block states of
  // the pinned Minecraft (block-states.js).
  chunkSnapshot(key, states, lod = null) {
    const chunk = this.chunks.get(key);
    if (!chunk) return null;
    const [cx, cy, cz] = coordsOf(key);
    const ox = cx * CHUNK_SIZE; const oy = cy * CHUNK_SIZE; const oz = cz * CHUNK_SIZE;
    const { entries } = this.palette;
    const { ids, occluding } = states.ofPalette(this.palette);
    const dense = chunk.view();
    // Face neighbours resolved once per chunk; boundary blocks look across.
    const px = this.chunks.get(keyOf(cx + 1, cy, cz));
    const nx = this.chunks.get(keyOf(cx - 1, cy, cz));
    const py = this.chunks.get(keyOf(cx, cy + 1, cz));
    const ny = this.chunks.get(keyOf(cx, cy - 1, cz));
    const pz = this.chunks.get(keyOf(cx, cy, cz + 1));
    const nz = this.chunks.get(keyOf(cx, cy, cz - 1));
    const across = (neighbor, index) => neighbor !== undefined && occluding[neighbor.get(index)] === 1;

    let records = [];
    for (let index = 0; index < CHUNK_VOLUME; index += 1) {
      const worldPaletteIndex = dense[index];
      if (worldPaletteIndex === 0) continue;
      const lx = index & 15;
      const lz = (index >> 4) & 15;
      const ly = index >> 8;
      const opaque = occluding[worldPaletteIndex] === 1;
      if (opaque
        && (lx < 15 ? occluding[dense[index + 1]] === 1 : across(px, index - 15))
        && (lx > 0 ? occluding[dense[index - 1]] === 1 : across(nx, index + 15))
        && (ly < 15 ? occluding[dense[index + 256]] === 1 : across(py, index - 15 * 256))
        && (ly > 0 ? occluding[dense[index - 256]] === 1 : across(ny, index + 15 * 256))
        && (lz < 15 ? occluding[dense[index + 16]] === 1 : across(pz, index - 15 * 16))
        && (lz > 0 ? occluding[dense[index - 16]] === 1 : across(nz, index + 15 * 16))) continue;
      records.push({ coords: [lx, ly, lz], index: worldPaletteIndex, opaque, scale: null });
    }

    if (lod && lod.scale > 1) {
      const planeAxes = [0, 1, 2].filter((axis) => axis !== lod.axis);
      const groups = new Map();
      for (const record of records) {
        const c = record.coords;
        const groupKey = `${c[lod.axis]},${Math.floor(c[planeAxes[0]] / lod.scale)},${Math.floor(c[planeAxes[1]] / lod.scale)}`;
        let group = groups.get(groupKey);
        if (!group) groups.set(groupKey, group = []);
        group.push(record);
      }
      const compressed = [];
      for (const group of groups.values()) {
        if (group.length !== lod.scale ** 2 || !group.every((record) => record.opaque)) {
          compressed.push(...group);
          continue;
        }
        const counts = new Map();
        let representative = group[0];
        for (const record of group) {
          const count = (counts.get(record.index) || 0) + 1;
          counts.set(record.index, count);
          if (count > (counts.get(representative.index) || 0)) representative = record;
        }
        const coords = [...representative.coords];
        coords[planeAxes[0]] = Math.floor(coords[planeAxes[0]] / lod.scale) * lod.scale;
        coords[planeAxes[1]] = Math.floor(coords[planeAxes[1]] / lod.scale) * lod.scale;
        const scale = [1, 1, 1];
        scale[planeAxes[0]] = lod.scale;
        scale[planeAxes[1]] = lod.scale;
        compressed.push({ ...representative, coords, scale });
      }
      records = compressed;
    }

    const palette = [];
    const paletteIndex = new Map();
    const blocks = [];
    for (const { coords: [lx, ly, lz], index, scale } of records) {
      let localPaletteIndex = paletteIndex.get(index);
      if (localPaletteIndex === undefined) {
        localPaletteIndex = palette.length;
        const { id, states: properties } = entries[index];
        palette.push({ id, states: properties, state: ids[index] });
        paletteIndex.set(index, localPaletteIndex);
      }
      blocks.push(scale
        ? [lx, ly, lz, localPaletteIndex, scale[0], scale[1], scale[2]]
        : [lx, ly, lz, localPaletteIndex]);
    }
    return { key, origin: [ox, oy, oz], version: this.shownVersion(key, states), palette, blocks };
  }

  // What chunkSnapshot's chunk is made from: the chunk's version, and which
  // cells of the layer each of the six chunks across its faces turns to it
  // have six full faces (by number, interned: equal numbers, equal layers;
  // no chunk there is a layer of none), as they hide its blocks on its edges.
  shownVersion(key, states) {
    const { occluding } = states.ofPalette(this.palette);
    const [cx, cy, cz] = coordsOf(key);
    const layer = (near, side) => {
      const chunk = this.chunks.get(near);
      return chunk ? boundaryLayers(chunk, occluding)[side] : EMPTY_LAYER;
    };
    return `${this.chunks.get(key).version} ${layer(keyOf(cx + 1, cy, cz), 0)} ${layer(keyOf(cx - 1, cy, cz), 1)} ${layer(keyOf(cx, cy + 1, cz), 2)}`
      + ` ${layer(keyOf(cx, cy - 1, cz), 3)} ${layer(keyOf(cx, cy, cz + 1), 4)} ${layer(keyOf(cx, cy, cz - 1), 5)}`;
  }

  finalize() {
    this.revision += 1;
    this.base = null;
    this.ownedChunks.clear();
    this.changedChunks.clear();
    return this;
  }

  // Writes the chunks not yet persisted, as serializeManifest does, without
  // building the manifest.
  persistChunks(writeChunk) {
    for (const chunk of this.chunks.values()) {
      if (!chunk.persistedHash) chunk.persistedHash = writeChunk(chunk.pairs());
    }
  }

  serializeManifest(writeChunk) {
    const chunks = [];
    for (const [key, chunk] of this.chunks) {
      if (!chunk.persistedHash) chunk.persistedHash = writeChunk(chunk.pairs());
      chunks.push([coordsOf(key).join(','), chunk.persistedHash]);
    }
    return {
      revision: this.revision,
      blockCount: this.blockCount,
      settleTicks: this.settleTicks,
      palette: this.palette.entries.slice(1),
      chunks,
    };
  }

  diff(before) {
    if (this.base !== before) throw new Error('World.diff requires a direct clone of the previous world');
    let added = 0; let removed = 0; let changed = 0;
    for (const key of this.changedChunks) {
      const originalChunk = before.chunks.get(key);
      const currentChunk = this.chunks.get(key);
      if (!originalChunk && !currentChunk) continue;
      if (!originalChunk) { added += currentChunk.count; continue; }
      if (!currentChunk) { removed += originalChunk.count; continue; }
      const original = originalChunk.view();
      const current = currentChunk.view();
      for (let index = 0; index < CHUNK_VOLUME; index += 1) {
        if (original[index] === current[index]) continue;
        if (original[index] === 0) added += 1;
        else if (current[index] === 0) removed += 1;
        else changed += 1;
      }
    }
    return { added, removed, changed };
  }

  static fromManifest(manifest, readChunk, chunkCache = new Map()) {
    const palette = new PaletteStore(manifest.palette);
    const chunks = new Map();
    for (const [keyText, hash] of manifest.chunks) {
      let chunk = chunkCache.get(hash);
      if (!chunk) {
        const pairs = readChunk(hash);
        chunk = new Chunk();
        if (pairs.length > DENSE_THRESHOLD) chunk.ensureDense();
        for (const [index, paletteIndex] of pairs) chunk.set(index, paletteIndex);
        chunk.persistedHash = hash;
        chunkCache.set(hash, chunk);
      }
      const [cx, cy, cz] = keyText.split(',').map(Number);
      chunks.set(keyOf(cx, cy, cz), chunk);
    }
    return new World({
      chunks,
      palette,
      blockCount: manifest.blockCount,
      revision: manifest.revision,
      settleTicks: manifest.settleTicks,
    });
  }
}

module.exports = {
  World, Chunk, PaletteStore, MAX_COORD, MAX_BLOCKS, CHUNK_SIZE, CHUNK_VOLUME, DENSE_THRESHOLD, KEY_OFFSET,
};
