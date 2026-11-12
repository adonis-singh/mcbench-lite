'use strict';

// Blueprints: what the browser's mesher draws (renderer/src/world.rs reads
// them), in McbenchEngine.write's format, big endian:
//   i32 palette entries, each: UTF id, i32 properties, each: UTF name, UTF value
//     (entry 0 is air)
//   i32 min x, y, z, max x, y, z of the blocks (Minecraft coordinates)
//   i32 sections, each: i32 section x, y, z, then 4096 u16 palette indices
//     in Minecraft's order (y, z, x)
// The engine writes a build's preview; this module writes a voxel world's (worldBlueprint).

const { World, Chunk, MAX_BLOCKS, MAX_COORD, CHUNK_VOLUME, DENSE_THRESHOLD } = require('./world');

const AIR = 'minecraft:air';

// `palette`: state names ("id[key=value,...]"), air first; `sections`:
// [[sx, sy, sz], Uint16Array of palette indices]; `bounds`: [min x, y, z,
// max x, y, z].
function encodeBlueprint(palette, sections, bounds) {
  const parts = [];
  const int = (value) => { const b = Buffer.alloc(4); b.writeInt32BE(value); parts.push(b); };
  const text = (value) => { const b = Buffer.from(value, 'utf8'); const n = Buffer.alloc(2); n.writeUInt16BE(b.length); parts.push(n, b); };
  int(palette.length);
  for (const name of palette) {
    const open = name.indexOf('[');
    text(open < 0 ? name : name.slice(0, open));
    const pairs = open < 0 ? [] : name.slice(open + 1, -1).split(',').map((pair) => pair.split('='));
    int(pairs.length);
    for (const [key, value] of pairs) { text(key); text(value); }
  }
  for (const value of bounds) int(value);
  int(sections.length);
  for (const [origin, indices] of sections) {
    for (const value of origin) int(value);
    const data = Buffer.alloc(8192);
    for (let i = 0; i < 4096; i += 1) data.writeUInt16BE(indices[i], i * 2);
    parts.push(data);
  }
  return Buffer.concat(parts);
}

// Gathers blocks into sections and a palette: `add(x, y, z, name)` for one
// non-air block, `addSection` for a section of them, then `encode()`. An
// empty world spans one position.
function collector() {
  const palette = [AIR];
  const index = new Map([[AIR, 0]]);
  const sections = new Map();
  const bounds = [Infinity, Infinity, Infinity, -Infinity, -Infinity, -Infinity];
  const intern = (name) => {
    let value = index.get(name);
    if (value === undefined) { value = palette.length; index.set(name, value); palette.push(name); }
    return value;
  };
  const section = (sx, sy, sz) => {
    const key = `${sx},${sy},${sz}`;
    let found = sections.get(key);
    if (!found) sections.set(key, found = [[sx, sy, sz], new Uint16Array(4096)]);
    return found[1];
  };
  const extend = (x, y, z) => {
    if (x < bounds[0]) bounds[0] = x;
    if (y < bounds[1]) bounds[1] = y;
    if (z < bounds[2]) bounds[2] = z;
    if (x > bounds[3]) bounds[3] = x;
    if (y > bounds[4]) bounds[4] = y;
    if (z > bounds[5]) bounds[5] = z;
  };
  return {
    add(x, y, z, name) {
      section(x >> 4, y >> 4, z >> 4)[(y & 15) << 8 | (z & 15) << 4 | (x & 15)] = intern(name);
      extend(x, y, z);
    },
    // `names`: the section's palette; `indices`: its 4096 indices into it,
    // or null when the section is all names[0].
    addSection([sx, sy, sz], names, indices) {
      const values = names.map((name) => (name === AIR ? 0 : intern(name)));
      const target = section(sx, sy, sz);
      for (let i = 0; i < 4096; i += 1) {
        const value = values[indices ? indices[i] : 0];
        if (!value) continue;
        target[i] = value;
        extend(sx * 16 + (i & 15), sy * 16 + (i >> 8), sz * 16 + ((i >> 4) & 15));
      }
    },
    encode() {
      return encodeBlueprint(palette, [...sections.values()], sections.size ? bounds : [0, 0, 0, 0, 0, 0]);
    },
  };
}

// Blocks given as [x, y, z, state name], at Minecraft coordinates.
function blocksBlueprint(blocks) {
  const collected = collector();
  for (const [x, y, z, name] of blocks) collected.add(x, y, z, name);
  return collected.encode();
}

// A voxel world (server/world.js) at Minecraft height: browser y + yOffset.
// Properties it leaves out are the block's defaults to the mesher, as they
// are to the engine.
function worldBlueprint(world, yOffset) {
  const blocks = [];
  world.forEach((x, y, z, entry) => {
    const properties = Object.entries(entry.states || {}).sort(([a], [b]) => (a < b ? -1 : 1));
    blocks.push([x, y + yOffset, z, properties.length ? `${entry.id}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]` : entry.id]);
  });
  return blocksBlueprint(blocks);
}

// A blueprint's blocks as "x,y,z" -> state name, properties sorted.
// Calls fn(x, y, z, id, properties, state) for each block of a blueprint
// that is not air; `state` is "id[a=b,...]" with sorted properties.
function forEachBlueprintBlock(bytes, fn) {
  let at = 0;
  const int = () => { const v = bytes.readInt32BE(at); at += 4; return v; };
  const text = () => { const n = bytes.readUInt16BE(at); at += 2; const v = bytes.toString('utf8', at, at + n); at += n; return v; };
  const palette = [];
  for (let entries = int(); entries > 0; entries -= 1) {
    const id = text();
    const properties = {};
    const pairs = [];
    for (let count = int(); count > 0; count -= 1) {
      const [name, value] = [text(), text()];
      properties[name] = value;
      pairs.push(`${name}=${value}`);
    }
    palette.push([id, properties, pairs.length ? `${id}[${pairs.sort().join(',')}]` : id]);
  }
  at += 24; // bounds
  for (let sections = int(); sections > 0; sections -= 1) {
    const [sx, sy, sz] = [int(), int(), int()];
    for (let i = 0; i < 4096; i += 1) {
      const value = bytes.readUInt16BE(at); at += 2;
      if (value) {
        const [id, properties, state] = palette[value];
        fn(sx * 16 + (i & 15), sy * 16 + (i >> 8), sz * 16 + ((i >> 4) & 15), id, properties, state);
      }
    }
  }
}

// The World of a blueprint's blocks, at browser height y - yOffset, interning
// states into `palette` (a new one when omitted) in order of first
// appearance. It is what setting every forEachBlueprintBlock block into a
// new World would make (same palette, same chunks, same sparse or dense
// storage), a section at a time: a section is read as one typed array, and
// its blocks are written straight into the chunk arrays.
function blueprintWorld(bytes, yOffset, palette) {
  const world = new World({ palette });
  let at = 0;
  const int = () => { const v = bytes.readInt32BE(at); at += 4; return v; };
  const text = () => { const n = bytes.readUInt16BE(at); at += 2; const v = bytes.toString('utf8', at, at + n); at += n; return v; };
  const entries = [];
  for (let count = int(); count > 0; count -= 1) {
    const id = text();
    const states = {};
    for (let pairs = int(); pairs > 0; pairs -= 1) {
      const name = text();
      states[name] = text();
    }
    entries.push({ id, states });
  }
  at += 24; // bounds
  // Blueprint palette index -> the world's, made on first use; air is 0.
  const target = new Int32Array(entries.length).fill(-1);
  target[0] = 0;
  const cells = new Uint16Array(CHUNK_VOLUME);
  const raw = Buffer.from(cells.buffer);
  const filled = new Map(); // chunk key -> { dense, count }
  let lastKey = null;
  let last = null;
  for (let sections = int(); sections > 0; sections -= 1) {
    const sx = int(); const sy = int(); const sz = int();
    bytes.copy(raw, 0, at, at + raw.length);
    at += raw.length;
    raw.swap16();
    const ox = sx * 16; const oy = sy * 16 - yOffset; const oz = sz * 16;
    // A section that reaches past the world's limits is checked block by block.
    const inside = ox >= -MAX_COORD && ox + 15 <= MAX_COORD && oy >= -MAX_COORD && oy + 15 <= MAX_COORD && oz >= -MAX_COORD && oz + 15 <= MAX_COORD;
    for (let i = 0; i < CHUNK_VOLUME; i += 1) {
      const value = cells[i];
      if (value === 0) continue;
      let index = target[value];
      if (index === -1) {
        const { id, states } = entries[value];
        index = target[value] = id === 'minecraft:air' ? 0 : world.palette.intern(id, states);
      }
      const x = ox + (i & 15); const y = oy + (i >> 8); const z = oz + ((i >> 4) & 15);
      if (!inside) world.checkCoord(x, y, z);
      if (index === 0) continue;
      const key = World.keyOf(x >> 4, y >> 4, z >> 4);
      if (key !== lastKey) {
        lastKey = key;
        last = filled.get(key);
        if (!last) filled.set(key, last = { dense: new Uint16Array(CHUNK_VOLUME), count: 0 });
      }
      const slot = ((y & 15) << 8) | ((z & 15) << 4) | (x & 15);
      if (last.dense[slot] === 0) last.count += 1;
      last.dense[slot] = index;
    }
  }
  for (const [key, { dense, count }] of filled) {
    const chunk = new Chunk();
    chunk.count = count;
    if (count > DENSE_THRESHOLD) {
      chunk.sparse = null;
      chunk.dense = dense;
    } else {
      for (let i = 0; i < CHUNK_VOLUME; i += 1) if (dense[i] !== 0) chunk.sparse.set(i, dense[i]);
    }
    world.chunks.set(key, chunk);
    world.ownedChunks.add(key);
    world.changedChunks.add(key);
    world.blockCount += count;
  }
  if (world.blockCount > MAX_BLOCKS) throw new Error(`Build exceeds the ${MAX_BLOCKS} block limit`);
  return world;
}

function readBlueprint(bytes) {
  const states = new Map();
  forEachBlueprintBlock(bytes, (x, y, z, id, properties, state) => states.set(`${x},${y},${z}`, state));
  return states;
}

module.exports = { encodeBlueprint, blocksBlueprint, worldBlueprint, readBlueprint, forEachBlueprintBlock, blueprintWorld };
