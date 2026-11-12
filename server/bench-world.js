'use strict';

// One world holding every build of a bench round side by side along x, so
// the comparison is a single map. Builds move by whole chunks: a chunk whose
// source is unchanged keeps its object across updates, and the live world
// patches only what a model actually changed.

const { World, Chunk, PaletteStore, CHUNK_SIZE, KEY_OFFSET } = require('./world');

const GAP_CHUNKS = 2;
const EMPTY_CHUNKS = 2;

// The world a model's run had at the end of `round` (1-based): the checkpoint
// taken before its next round's prompt, or its current world.
function worldAtRound(member, round) {
  const prompts = member.messages.filter((message) => message.role === 'user' && !message.seeBuild);
  const next = prompts[round];
  return next ? member.checkpoints.get(next.id) : member.world;
}

function chunkSpan(bounds) {
  if (!bounds) return null;
  return {
    min: [bounds.min[0] >> 4, bounds.min[2] >> 4],
    max: [bounds.max[0] >> 4, bounds.max[2] >> 4],
  };
}

class BenchComposer {
  constructor() {
    this.palette = new PaletteStore();
    this.remaps = new Map(); // source palette -> composite indexes, extended lazily
    this.chunks = new Map(); // `${member}:${key}:${dx}:${dz}` -> { source, chunk }
    this.revision = 0;
  }

  remap(palette, index) {
    let table = this.remaps.get(palette);
    if (!table) this.remaps.set(palette, table = []);
    if (table[index] === undefined) {
      const entry = palette.get(index);
      table[index] = this.palette.intern(entry.id, entry.states);
    }
    return table[index];
  }

  moved(memberId, world, key, source, dx, dz) {
    const cacheKey = `${memberId}:${key}:${dx}:${dz}`;
    this.used.add(cacheKey);
    const cached = this.chunks.get(cacheKey);
    if (cached?.source === source) return cached.chunk;
    const chunk = new Chunk();
    if (source.dense) {
      chunk.dense = new Uint16Array(source.dense.length);
      for (let i = 0; i < source.dense.length; i += 1) {
        if (source.dense[i]) chunk.dense[i] = this.remap(world.palette, source.dense[i]);
      }
      chunk.sparse = null;
    } else {
      for (const [index, value] of source.sparse) chunk.sparse.set(index, this.remap(world.palette, value));
    }
    chunk.count = source.count;
    this.chunks.set(cacheKey, { source, chunk });
    return chunk;
  }

  // entries: [{ id, world }] in display order.
  compose(entries) {
    const spans = entries.map(({ world }) => chunkSpan(world.bounds()));
    const widths = spans.map((span) => (span ? span.max[0] - span.min[0] + 1 : EMPTY_CHUNKS));
    const total = widths.reduce((sum, width) => sum + width, 0) + GAP_CHUNKS * (entries.length - 1);
    let cursor = -Math.floor(total / 2);
    this.used = new Set();
    const chunks = new Map();
    const layout = [];
    let blockCount = 0;
    entries.forEach(({ id, world }, slot) => {
      const span = spans[slot];
      const dx = span ? cursor - span.min[0] : 0;
      const dz = span ? -Math.round((span.min[1] + span.max[1]) / 2) : 0;
      if (span) {
        for (const [key, source] of world.chunks) {
          const [cx, cy, cz] = World.coordsOf(key);
          const x = cx + dx;
          const z = cz + dz;
          if (x < -KEY_OFFSET || x >= KEY_OFFSET || z < -KEY_OFFSET || z >= KEY_OFFSET) {
            throw new Error('These builds are too large to show side by side');
          }
          chunks.set(World.keyOf(x, cy, z), this.moved(id, world, key, source, dx, dz));
        }
        const bounds = world.bounds();
        layout.push({
          id,
          min: [bounds.min[0] + dx * CHUNK_SIZE, bounds.min[1], bounds.min[2] + dz * CHUNK_SIZE],
          max: [bounds.max[0] + dx * CHUNK_SIZE, bounds.max[1], bounds.max[2] + dz * CHUNK_SIZE],
        });
        blockCount += world.size();
      } else {
        const x = cursor * CHUNK_SIZE;
        layout.push({ id, min: [x, 0, -CHUNK_SIZE], max: [x + EMPTY_CHUNKS * CHUNK_SIZE - 1, 0, CHUNK_SIZE - 1] });
      }
      cursor += widths[slot] + GAP_CHUNKS;
    });
    for (const key of this.chunks.keys()) if (!this.used.has(key)) this.chunks.delete(key);
    this.revision += 1;
    return { world: new World({ chunks, palette: this.palette, blockCount, revision: this.revision }), layout };
  }
}

module.exports = { BenchComposer, worldAtRound };
