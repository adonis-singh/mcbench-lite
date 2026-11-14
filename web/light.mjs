const LIGHT_CAP = 16_000_000;
// Light falls by one a step from at most 15, so a cell lit through a path
// of 15 steps or more gets nothing from it: a change reaches 14 steps.
const REACH = 14;
const NO_TOP = -2147483648;

// The light of a world: a voxel field over the blocks' bounding box (+1 air
// margin), skylight 15 down every column above its highest opaque block,
// block light from emitters, and both spread with -1 a step through cells
// that are not opaque. A cell's level is the largest of its own source
// (15 for open sky, its emitter's level) and its neighbours' levels less
// one where it is not opaque: the smallest field that holds this, which a
// work queue reaches from the sources in any order.
//
// `createLight()` holds that field and what it is made from (each cell's
// opacity and emission, each column's highest opaque block, each chunk's
// block extents). `updateLight(light, world, touched)` brings it to `world`
// after the blocks of the chunks in `touched` ('cx,cy,cz') changed, and
// answers the chunks holding a cell whose level changed. Only cells within
// REACH steps of a cell whose opacity, emission or open sky changed can
// change: the cells a level comes from lie on a path of at most REACH steps
// to it. Open sky is not local: an opaque block put up or taken away opens
// or closes the sky of its column down to the next opaque block below (the
// span between the column's old and new highest opaque block), so those
// cells count as changed too. A box that grew holds cells that were air
// under open sky (no block was past the old box's sides or above it), and
// they keep that light unless a change is near; below the old box the
// light of the cells under the build was not held, and those count as
// changed. A box that shrank counts the cells of its sides that moved in as
// changed. Those cells, and the cells within
// REACH of them, are lit again from their sources and from the levels of
// the cells around them, which keep theirs: the same field the whole world
// lit from nothing gives (test/light.test.js compares the two after random
// edits). With nothing held (the first update, after a reset, or after a
// box past LIGHT_CAP), every cell has changed: one path lights a whole
// world and an edit.
//
// `world`: its chunks ('cx,cy,cz' -> Map of placed blocks, each with pos,
// whether it is a full opaque cube, and its light level lum, which the
// viewer's world also keeps as its light sources).
export function createLight() {
  return { field: null, grid: null, chunkBounds: new Map(), scratch: null };
}

// The light of `world` lit from nothing, as a field (null when there is
// none: an empty world, or one past LIGHT_CAP).
export function computeLight(world) {
  const light = createLight();
  updateLight(light, world, world.chunks.keys());
  return light.field;
}

function chunkExtent(chunk) {
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const { pos } of chunk.values()) {
    if (pos[0] < minX) minX = pos[0]; if (pos[0] > maxX) maxX = pos[0];
    if (pos[1] < minY) minY = pos[1]; if (pos[1] > maxY) maxY = pos[1];
    if (pos[2] < minZ) minZ = pos[2]; if (pos[2] > maxZ) maxZ = pos[2];
  }
  return [minX, minY, minZ, maxX, maxY, maxZ];
}

// The box of the cells lit: the blocks' extents and a cell more on each side.
function boxOf(chunkBounds) {
  if (chunkBounds.size === 0) return null;
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const b of chunkBounds.values()) {
    if (b[0] < minX) minX = b[0]; if (b[1] < minY) minY = b[1]; if (b[2] < minZ) minZ = b[2];
    if (b[3] > maxX) maxX = b[3]; if (b[4] > maxY) maxY = b[4]; if (b[5] > maxZ) maxZ = b[5];
  }
  minX -= 1; minY -= 1; minZ -= 1; maxX += 1; maxY += 1; maxZ += 1;
  const sx = maxX - minX + 1, sy = maxY - minY + 1, sz = maxZ - minZ + 1;
  if (sx * sy * sz > LIGHT_CAP) return null;
  return { minX, minY, minZ, maxX, maxY, maxZ, sx, sy, sz, vol: sx * sy * sz };
}

const sameBox = (a, b) => a.minX === b.minX && a.minY === b.minY && a.minZ === b.minZ && a.maxX === b.maxX && a.maxY === b.maxY && a.maxZ === b.maxZ;
const inside = (g, x, y, z) => x >= g.minX && x <= g.maxX && y >= g.minY && y <= g.maxY && z >= g.minZ && z <= g.maxZ;

// A grid over `box`, holding what `old` held where the two overlap.
function regrid(old, box) {
  const g = { ...box, solid: new Uint8Array(box.vol), emit: new Uint8Array(box.vol), lvl: new Uint8Array(box.vol).fill(15), top: new Int32Array(box.sx * box.sz).fill(NO_TOP) };
  if (!old) return g;
  const x0 = Math.max(old.minX, g.minX), x1 = Math.min(old.maxX, g.maxX);
  const y0 = Math.max(old.minY, g.minY), y1 = Math.min(old.maxY, g.maxY);
  const z0 = Math.max(old.minZ, g.minZ), z1 = Math.min(old.maxZ, g.maxZ);
  if (x0 > x1 || y0 > y1 || z0 > z1) return g;
  const width = x1 - x0 + 1;
  for (let y = y0; y <= y1; y++) {
    for (let z = z0; z <= z1; z++) {
      const from = ((y - old.minY) * old.sz + (z - old.minZ)) * old.sx + (x0 - old.minX);
      const to = ((y - g.minY) * g.sz + (z - g.minZ)) * g.sx + (x0 - g.minX);
      for (const name of ['solid', 'emit', 'lvl']) g[name].set(old[name].subarray(from, from + width), to);
    }
  }
  for (let z = z0; z <= z1; z++) {
    const from = (z - old.minZ) * old.sx + (x0 - old.minX);
    g.top.set(old.top.subarray(from, from + width), (z - g.minZ) * g.sx + (x0 - g.minX));
  }
  return g;
}

// Calls fn(x, y, z) for each cell of box `a` that is not in box `b` (b may
// be null), a row at a time.
function forEachOutside(a, b, fn) {
  for (let y = a.minY; y <= a.maxY; y++) {
    for (let z = a.minZ; z <= a.maxZ; z++) {
      const rowIn = b && y >= b.minY && y <= b.maxY && z >= b.minZ && z <= b.maxZ;
      if (!rowIn) {
        for (let x = a.minX; x <= a.maxX; x++) fn(x, y, z);
        continue;
      }
      for (let x = a.minX; x < Math.min(b.minX, a.maxX + 1); x++) fn(x, y, z);
      for (let x = Math.max(b.maxX + 1, a.minX); x <= a.maxX; x++) fn(x, y, z);
    }
  }
}

function fieldOf(g) {
  const { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, lvl } = g;
  const at = (x, y, z) => ((y - minY) * sz + (z - minZ)) * sx + (x - minX);
  return { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, lvl, at };
}

export function updateLight(light, world, touched) {
  const keys = new Set(touched);
  for (const key of keys) {
    const chunk = world.chunks.get(key);
    if (chunk && chunk.size > 0) light.chunkBounds.set(key, chunkExtent(chunk));
    else light.chunkBounds.delete(key);
  }
  const old = light.grid;
  const box = boxOf(light.chunkBounds);
  const changed = new Set();
  let lastX = NaN, lastY = NaN, lastZ = NaN;
  const changedAt = (x, y, z) => {
    const cx = x >> 4, cy = y >> 4, cz = z >> 4;
    if (cx === lastX && cy === lastY && cz === lastZ) return;
    lastX = cx; lastY = cy; lastZ = cz;
    changed.add(`${cx},${cy},${cz}`);
  };
  // Cells the new field no longer holds read as direct skylight.
  if (old) {
    forEachOutside(old, box, (x, y, z) => {
      if (old.lvl[((y - old.minY) * old.sz + (z - old.minZ)) * old.sx + (x - old.minX)] !== 15) changedAt(x, y, z);
    });
  }
  if (!box) {
    light.grid = null;
    light.field = null;
    return changed;
  }
  const g = old && sameBox(old, box) ? old : regrid(old, box);
  const { minX, minY, minZ, maxX, maxY, maxZ, sx, sz, vol } = g;
  const plane = sx * sz;
  const at = (x, y, z) => ((y - minY) * sz + (z - minZ)) * sx + (x - minX);
  if (!light.scratch || light.scratch.vol !== vol) {
    light.scratch = { vol, reach: new Uint8Array(vol), queue: new Uint32Array(vol), queued: new Uint8Array(vol), cells: new Uint32Array(vol), levels: new Uint8Array(vol) };
  }
  const { reach, queue, queued, cells, levels } = light.scratch;
  // reach[i]: 0 not near a change, else REACH + 1 less its fewest steps from
  // one. `cells` lists the cells near a change, each once, in the order they
  // are reached: every change first, then a step further at a time, so a
  // cell is reached first by its fewest steps.
  let count = 0;
  let head = 0; let tail = 0; let pending = 0;
  const enqueue = (index) => {
    if (queued[index]) return;
    queued[index] = 1;
    queue[tail] = index;
    tail = (tail + 1) % vol;
    pending += 1;
  };
  const dequeue = () => {
    const index = queue[head];
    head = (head + 1) % vol;
    pending -= 1;
    queued[index] = 0;
    return index;
  };
  const nearBy = (index, level) => {
    if (reach[index]) return;
    reach[index] = level;
    cells[count++] = index;
  };
  const seed = (index) => nearBy(index, REACH + 1);

  // Cells new to the box are air that was open sky (direct skylight): past
  // the old box's sides and top no block was, so each column there was open
  // and every cell above the old box was under open sky. Below it the old
  // light of the cells under the build was not held: those count as changed,
  // as every cell does with nothing held. A box that shrank changed the
  // cells on its sides that shrank, the box's cells nearest the cells it
  // dropped.
  if (!old) {
    reach.fill(REACH + 1);
    for (let i = 0; i < vol; i++) cells[i] = i;
    count = vol;
  } else if (old !== g) {
    forEachOutside(g, old, (x, y, z) => {
      if (y < old.minY && x >= old.minX && x <= old.maxX && z >= old.minZ && z <= old.maxZ) seed(at(x, y, z));
    });
    const face = (axis, value) => {
      const lo = [minX, minY, minZ], hi = [maxX, maxY, maxZ];
      lo[axis] = hi[axis] = value;
      for (let y = lo[1]; y <= hi[1]; y++) for (let z = lo[2]; z <= hi[2]; z++) for (let x = lo[0]; x <= hi[0]; x++) seed(at(x, y, z));
    };
    if (old.minX < minX) face(0, minX);
    if (old.maxX > maxX) face(0, maxX);
    if (old.minY < minY) face(1, minY);
    if (old.maxY > maxY) face(1, maxY);
    if (old.minZ < minZ) face(2, minZ);
    if (old.maxZ > maxZ) face(2, maxZ);
  }
  // The touched chunks' cells (all chunks when nothing was held): their
  // opacity and emission now, and the cells where either changed.
  const read = old ? keys : new Set(world.chunks.keys());
  const columns = new Set();
  const wasSolid = new Uint8Array(4096);
  const wasEmit = new Uint8Array(4096);
  for (const key of read) {
    const [cx, cy, cz] = key.split(',').map(Number);
    const x0 = Math.max(minX, cx * 16), x1 = Math.min(maxX, cx * 16 + 15);
    const y0 = Math.max(minY, cy * 16), y1 = Math.min(maxY, cy * 16 + 15);
    const z0 = Math.max(minZ, cz * 16), z1 = Math.min(maxZ, cz * 16 + 15);
    if (x0 > x1 || z0 > z1) continue;
    // A chunk past the box's height still held the highest opaque block of
    // its columns before its blocks went.
    for (let z = z0; z <= z1; z++) for (let x = x0; x <= x1; x++) columns.add((z - minZ) * sx + (x - minX));
    if (y0 > y1) continue;
    const chunk = world.chunks.get(key);
    // A grid made now holds nothing yet, and every cell has changed.
    if (!old) {
      for (const { pos, opaque, lum } of chunk.values()) {
        const i = at(pos[0], pos[1], pos[2]);
        if (opaque) g.solid[i] = 1;
        if (lum > 0) g.emit[i] = lum;
      }
      continue;
    }
    let n = 0;
    for (let y = y0; y <= y1; y++) for (let z = z0; z <= z1; z++) {
      const row = at(x0, y, z);
      for (let i = row; i <= row + x1 - x0; i++) {
        wasSolid[n] = g.solid[i];
        wasEmit[n++] = g.emit[i];
        g.solid[i] = 0;
        g.emit[i] = 0;
      }
    }
    // Emission is each block's light level, which the world's light sources
    // (emitters) hold for every block that gives light.
    if (chunk) {
      for (const { pos, opaque, lum } of chunk.values()) {
        const i = at(pos[0], pos[1], pos[2]);
        if (opaque) g.solid[i] = 1;
        if (lum > 0) g.emit[i] = lum;
      }
    }
    // With every cell changed already, there is nothing to compare.
    if (count === vol) continue;
    n = 0;
    for (let y = y0; y <= y1; y++) for (let z = z0; z <= z1; z++) {
      const row = at(x0, y, z);
      for (let i = row; i <= row + x1 - x0; i++, n++) if (g.solid[i] !== wasSolid[n] || g.emit[i] !== wasEmit[n]) seed(i);
    }
  }
  // Each read column's highest opaque block, and the span whose open sky it
  // opened or closed.
  for (const column of columns) {
    let top = NO_TOP;
    for (let y = maxY; y >= minY; y--) {
      if (g.solid[(y - minY) * plane + column]) { top = y; break; }
    }
    const was = g.top[column];
    if (was === top) continue;
    g.top[column] = top;
    const low = Math.max(Math.min(was, top), minY - 1);
    // Within the box: a highest block above it left with blocks the box dropped.
    const high = Math.min(Math.max(was, top), maxY);
    for (let y = low + 1; y <= high; y++) seed((y - minY) * plane + column);
  }

  // The cells within REACH steps of a change (steps through any cell); with
  // every cell changed, there is nothing to look for.
  if (count < vol) {
    for (let n = 0; n < count; n++) {
      const index = cells[n];
      const next = reach[index] - 1;
      if (next === 0) continue;
      const x = index % sx;
      const z = Math.floor(index / sx) % sz;
      if (x + 1 < sx) nearBy(index + 1, next);
      if (x > 0) nearBy(index - 1, next);
      if (index + plane < vol) nearBy(index + plane, next);
      if (index >= plane) nearBy(index - plane, next);
      if (z + 1 < sz) nearBy(index + sx, next);
      if (z > 0) nearBy(index - sx, next);
    }
  }
  // Their levels before (the old field's, which the grid holds until a cell
  // is lit again, and direct skylight for a cell new to the box), and each
  // lit from its own source.
  for (let n = 0; n < count; n++) {
    const index = cells[n];
    const y = ((index / plane) | 0) + minY;
    let level = 15;
    if (old) {
      const rest = index % plane;
      const z = ((rest / sx) | 0) + minZ;
      const x = rest % sx + minX;
      if (inside(old, x, y, z)) level = g.lvl[index];
    }
    levels[n] = level;
    const lit = Math.max(y > g.top[index % plane] ? 15 : 0, g.emit[index]);
    g.lvl[index] = lit;
    if (lit > 0) enqueue(index);
  }
  // And from the cells around them, which keep their levels (none when
  // every cell was lit again).
  for (let n = 0; n < (count === vol ? 0 : count); n++) {
    const index = cells[n];
    const x = index % sx;
    const z = Math.floor(index / sx) % sz;
    if (x + 1 < sx && !reach[index + 1] && g.lvl[index + 1] > 1) enqueue(index + 1);
    if (x > 0 && !reach[index - 1] && g.lvl[index - 1] > 1) enqueue(index - 1);
    if (index + plane < vol && !reach[index + plane] && g.lvl[index + plane] > 1) enqueue(index + plane);
    if (index >= plane && !reach[index - plane] && g.lvl[index - plane] > 1) enqueue(index - plane);
    if (z + 1 < sz && !reach[index + sx] && g.lvl[index + sx] > 1) enqueue(index + sx);
    if (z > 0 && !reach[index - sx] && g.lvl[index - sx] > 1) enqueue(index - sx);
  }
  const { solid, lvl } = g;
  while (pending > 0) {
    const index = dequeue();
    const next = lvl[index] - 1;
    if (next <= 0) continue;
    const x = index % sx;
    const z = Math.floor(index / sx) % sz;
    if (x + 1 < sx && !solid[index + 1] && lvl[index + 1] < next) { lvl[index + 1] = next; enqueue(index + 1); }
    if (x > 0 && !solid[index - 1] && lvl[index - 1] < next) { lvl[index - 1] = next; enqueue(index - 1); }
    if (index + plane < vol && !solid[index + plane] && lvl[index + plane] < next) { lvl[index + plane] = next; enqueue(index + plane); }
    if (index >= plane && !solid[index - plane] && lvl[index - plane] < next) { lvl[index - plane] = next; enqueue(index - plane); }
    if (z + 1 < sz && !solid[index + sx] && lvl[index + sx] < next) { lvl[index + sx] = next; enqueue(index + sx); }
    if (z > 0 && !solid[index - sx] && lvl[index - sx] < next) { lvl[index - sx] = next; enqueue(index - sx); }
  }
  for (let n = 0; n < count; n++) {
    const index = cells[n];
    reach[index] = 0;
    if (lvl[index] === levels[n]) continue;
    const rest = index % plane;
    changedAt(rest % sx + minX, ((index / plane) | 0) + minY, ((rest / sx) | 0) + minZ);
  }
  light.grid = g;
  light.field = fieldOf(g);
  return changed;
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
