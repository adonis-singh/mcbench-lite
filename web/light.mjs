const LIGHT_CAP = 16_000_000;

// `world`: its block count (size), its blocks (forEachBlock: each with its
// position and whether it is a full opaque cube) and its light sources
// (emitters: "x,y,z" -> level).
export function computeLight(world) {
  if (world.size === 0) return null;
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  world.forEachBlock(({ pos }) => {
    if (pos[0] < minX) minX = pos[0]; if (pos[0] > maxX) maxX = pos[0];
    if (pos[1] < minY) minY = pos[1]; if (pos[1] > maxY) maxY = pos[1];
    if (pos[2] < minZ) minZ = pos[2]; if (pos[2] > maxZ) maxZ = pos[2];
  });
  minX -= 1; minY -= 1; minZ -= 1; maxX += 1; maxY += 1; maxZ += 1;
  const sx = maxX - minX + 1, sy = maxY - minY + 1, sz = maxZ - minZ + 1;
  if (sx * sy * sz > LIGHT_CAP) return null;
  const vol = sx * sy * sz;
  const at = (x, y, z) => ((y - minY) * sz + (z - minZ)) * sx + (x - minX);
  const solid = new Uint8Array(vol);
  world.forEachBlock(({ pos, opaque }) => {
    if (opaque) solid[at(pos[0], pos[1], pos[2])] = 1;
  });
  const lvl = new Uint8Array(vol);
  // At most one pending entry per cell. Queue indexes replace coordinate
  // triples and per-visit neighbor arrays; memory stays bounded by the grid.
  const queue = new Uint32Array(vol);
  const queued = new Uint8Array(vol);
  let head = 0; let tail = 0; let pending = 0;
  const enqueue = (index) => {
    if (queued[index]) return;
    queued[index] = 1;
    queue[tail] = index;
    tail = (tail + 1) % vol;
    pending += 1;
  };
  for (let x = minX; x <= maxX; x++) {
    for (let z = minZ; z <= maxZ; z++) {
      for (let y = maxY; y >= minY; y--) {
        const i = at(x, y, z);
        if (solid[i]) break;
        lvl[i] = 15;
        enqueue(i);
      }
    }
  }
  for (const [key, lum] of world.emitters) {
    const [x, y, z] = key.split(',').map(Number);
    const i = at(x, y, z);
    if (lum > lvl[i]) {
      lvl[i] = lum;
      enqueue(i);
    }
  }
  const spread = (index, level) => {
    if (solid[index] || lvl[index] >= level) return;
    lvl[index] = level;
    enqueue(index);
  };
  const plane = sx * sz;
  while (pending > 0) {
    const index = queue[head];
    head = (head + 1) % vol;
    pending -= 1;
    queued[index] = 0;
    const next = lvl[index] - 1;
    if (next <= 0) continue;
    const x = index % sx;
    const z = Math.floor(index / sx) % sz;
    if (x + 1 < sx) spread(index + 1, next);
    if (x > 0) spread(index - 1, next);
    if (index + plane < vol) spread(index + plane, next);
    if (index >= plane) spread(index - plane, next);
    if (z + 1 < sz) spread(index + sx, next);
    if (z > 0) spread(index - sx, next);
  }
  return { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, lvl, at };
}

// A light field's level at x, y, z: computeLight's, where null is direct
// skylight, and 15 outside the field.
export function lightAt(field, x, y, z) {
  if (!field || x < field.minX || x > field.maxX || y < field.minY || y > field.maxY || z < field.minZ || z > field.maxZ) return 15;
  return field.lvl[field.at(x, y, z)];
}

// The 16-block chunks ('cx,cy,cz') holding a cell whose level differs
// between two light fields (computeLight's, either of them null).
export function changedLightChunks(before, after) {
  const changed = new Set();
  const fields = [before, after].filter(Boolean);
  if (fields.length === 0) return changed;
  const minX = Math.min(...fields.map((field) => field.minX)), maxX = Math.max(...fields.map((field) => field.maxX));
  const minY = Math.min(...fields.map((field) => field.minY)), maxY = Math.max(...fields.map((field) => field.maxY));
  const minZ = Math.min(...fields.map((field) => field.minZ)), maxZ = Math.max(...fields.map((field) => field.maxZ));
  for (let cy = minY >> 4; cy <= maxY >> 4; cy++) {
    for (let cz = minZ >> 4; cz <= maxZ >> 4; cz++) {
      for (let cx = minX >> 4; cx <= maxX >> 4; cx++) {
        const x1 = Math.min(maxX, cx * 16 + 15), y1 = Math.min(maxY, cy * 16 + 15), z1 = Math.min(maxZ, cz * 16 + 15);
        scan: for (let y = Math.max(minY, cy * 16); y <= y1; y++) {
          for (let z = Math.max(minZ, cz * 16); z <= z1; z++) {
            for (let x = Math.max(minX, cx * 16); x <= x1; x++) {
              if (lightAt(before, x, y, z) !== lightAt(after, x, y, z)) {
                changed.add(`${cx},${cy},${cz}`);
                break scan;
              }
            }
          }
        }
      }
    }
  }
  return changed;
}
