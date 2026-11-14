const LIGHT_CAP = 16_000_000;
// Light falls by one a step or more from at most 15, so a cell lit through a
// path of 15 steps or more gets nothing from it: a change reaches 14 steps.
const REACH = 14;
const NO_TOP = -2147483648;
// A light class's step: the light it takes from a neighbour (bits 0-3),
// whether it shapes light with its faces (SHAPED), whether sky light falls
// straight down through it (SKY) and whether it emits light (EMITS).
const SHAPED = 16;
const SKY = 32;
const EMITS = 64;
// Direction order (Minecraft's Direction): down, up, north, south, west, east.
const DOWN = 0, UP = 1, NORTH = 2, SOUTH = 3, WEST = 4, EAST = 5;

// Minecraft's light of a build standing alone: open sky above it and on
// every side, air below it down to the world's floor (`floor`, the lowest
// height a block can have) and nothing past that, which is the world the live
// viewer's mesher lights a blueprint in (renderer/src/world.rs: its volume
// stops at the floor and reads full sky light past its edges). What a cell
// holds is the brighter of its sky and block light, in full daylight. The
// rule is Paper's light engine's (Starlight, which core/src/light/engine.rs
// ports line for line), with each block state's facts from Minecraft's block
// table (server/block-states.js):
//   - Sky light is 15 from above down a column through every cell that
//     blocks no light (light block 0) and whose face shapes towards the
//     cell above do not close (below).
//   - A cell emitting light holds its emission.
//   - Light steps to each neighbour less the neighbour's light block, at
//     least 1, unless the face shapes the two cells turn to each other
//     close (Shapes.faceShapeOccludes; a cell that is not conditionally full
//     opaque turns an empty face).
// A cell's level is the largest of its own sources and what its neighbours
// give it: the smallest field that holds this, which a work queue reaches
// from the sources in any order (the same field for sky and block light
// spread apart and the brighter taken: both spread by the same steps).
// The field covers the blocks' bounding box and one air cell more on each
// side, above the floor: past it a column holds no block, so its sky is
// open, and below it a path through the air under the build is never
// shorter than one along the box's bottom layer, so the field is the same as
// one lit over any larger box. Every cell outside the field reads 15.
//
// Each block state lights as its light class (`lightRule`): states with the
// same emission, step, sky passing and face shapes are one class, and empty
// space is class 0, air's.
//
// `createLight(rule, floor)` holds that field and what it is made from (each
// cell's light class, each column's highest cell the sky does not reach,
// each chunk's block extents). `updateLight(light, world, touched)` brings it
// to `world` after the blocks of the chunks in `touched` ('cx,cy,cz')
// changed, and answers the chunks holding a cell whose level changed. Only
// cells within REACH steps of a cell whose light class or open sky changed
// can change: the cells a level comes from lie on a path of at most REACH
// steps to it. Open sky is not local: a block put up or taken away opens or
// closes the sky of its column down to the next cell the sky stops at (the
// span between the column's old and new highest cell the sky does not
// reach), so those cells count as changed too. A box that grew holds cells
// that were air under open sky (no block was past the old box's sides or
// above it), and they keep that light unless a change is near; below the old
// box the light of the cells under the build was not held, and those count
// as changed. A box that shrank counts the cells of its sides that moved in
// as changed. Those cells, and the cells within REACH of them, are lit again
// from their sources and from the levels of the cells around them, which
// keep theirs: the same field the whole world lit from nothing gives
// (test/light.test.js compares the two after random edits). With nothing
// held (the first update, after a reset, or after a box past LIGHT_CAP),
// every cell has changed: one path lights a whole world and an edit.
//
// `world`: its chunks ('cx,cy,cz' -> Map of placed blocks, each with pos and
// its light class, `light`).
export function createLight(rule, floor) {
  return { rule, floor, field: null, grid: null, chunkBounds: new Map(), scratch: null };
}

// The light classes of Minecraft's block states, from their facts by state
// id (server/block-states.js): `emission`, `lightBlock`, `faces` (six face
// shapes a state, 0 the empty face) and `occludes` (by pair of the `shapes`
// face shapes, row by row). Answers the rule createLight lights with, and
// `classOf`, each state's class. State 0 is air.
export function lightRule({ emission, lightBlock, faces, shapes, occludes }) {
  const count = emission.length;
  const classOf = new Uint16Array(count);
  const keys = new Map();
  const classes = [];
  for (let state = 0; state < count; state++) {
    const shape = faces.subarray(state * 6, state * 6 + 6);
    const step = Math.max(1, lightBlock[state]) | (shape.some((face) => face !== 0) ? SHAPED : 0) | (lightBlock[state] === 0 ? SKY : 0) | (emission[state] > 0 ? EMITS : 0);
    const key = `${emission[state]} ${step} ${shape.join(',')}`;
    let light = keys.get(key);
    if (light === undefined) {
      light = classes.length;
      if (light > 0xffff) throw new Error('More light classes than a cell holds');
      keys.set(key, light);
      classes.push({ emission: emission[state], step, shape });
    }
    classOf[state] = light;
  }
  const face = new Uint8Array(classes.length * 6);
  classes.forEach(({ shape }, light) => face.set(shape, light * 6));
  return {
    classOf,
    emission: Uint8Array.from(classes, ({ emission: level }) => level),
    step: Uint8Array.from(classes, ({ step }) => step),
    face,
    shapes,
    occludes,
  };
}

// The light of `world` lit from nothing, as a field (null when there is
// none: an empty world, or one past LIGHT_CAP).
export function computeLight(rule, floor, world) {
  const light = createLight(rule, floor);
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

// The box of the cells lit: the blocks' extents and a cell more on each
// side, none below the floor.
function boxOf(chunkBounds, floor) {
  if (chunkBounds.size === 0) return null;
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const b of chunkBounds.values()) {
    if (b[0] < minX) minX = b[0]; if (b[1] < minY) minY = b[1]; if (b[2] < minZ) minZ = b[2];
    if (b[3] > maxX) maxX = b[3]; if (b[4] > maxY) maxY = b[4]; if (b[5] > maxZ) maxZ = b[5];
  }
  minX -= 1; minY = Math.max(minY - 1, floor); minZ -= 1; maxX += 1; maxY += 1; maxZ += 1;
  const sx = maxX - minX + 1, sy = maxY - minY + 1, sz = maxZ - minZ + 1;
  if (sx * sy * sz > LIGHT_CAP) return null;
  return { minX, minY, minZ, maxX, maxY, maxZ, sx, sy, sz, vol: sx * sy * sz };
}

const sameBox = (a, b) => a.minX === b.minX && a.minY === b.minY && a.minZ === b.minZ && a.maxX === b.maxX && a.maxY === b.maxY && a.maxZ === b.maxZ;
const inside = (g, x, y, z) => x >= g.minX && x <= g.maxX && y >= g.minY && y <= g.maxY && z >= g.minZ && z <= g.maxZ;

// A grid over `box`, holding what `old` held where the two overlap: each
// cell's light class (`cls`, empty space air's, 0), its class's step
// (`steps`, read where the light spreads without looking the class up),
// its level (`lvl`) and each column's highest cell the sky does not reach.
function regrid(old, box, airStep) {
  const g = { ...box, cls: new Uint16Array(box.vol), steps: new Uint8Array(box.vol).fill(airStep), lvl: new Uint8Array(box.vol).fill(15), top: new Int32Array(box.sx * box.sz).fill(NO_TOP) };
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
      g.cls.set(old.cls.subarray(from, from + width), to);
      g.steps.set(old.steps.subarray(from, from + width), to);
      g.lvl.set(old.lvl.subarray(from, from + width), to);
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
  const box = boxOf(light.chunkBounds, light.floor);
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
  const g = old && sameBox(old, box) ? old : regrid(old, box, light.rule.step[0]);
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
  // light classes now, and the cells whose class changed.
  const { rule } = light;
  const { emission, step, face, shapes, occludes } = rule;
  const { cls, steps } = g;
  const airStep = step[0];
  const read = old ? keys : new Set(world.chunks.keys());
  const columns = new Set();
  const was = new Uint16Array(4096);
  for (const key of read) {
    const [cx, cy, cz] = key.split(',').map(Number);
    const x0 = Math.max(minX, cx * 16), x1 = Math.min(maxX, cx * 16 + 15);
    const y0 = Math.max(minY, cy * 16), y1 = Math.min(maxY, cy * 16 + 15);
    const z0 = Math.max(minZ, cz * 16), z1 = Math.min(maxZ, cz * 16 + 15);
    if (x0 > x1 || z0 > z1) continue;
    // A chunk past the box's height still held the highest cell the sky
    // did not reach in its columns before its blocks went.
    for (let z = z0; z <= z1; z++) for (let x = x0; x <= x1; x++) columns.add((z - minZ) * sx + (x - minX));
    if (y0 > y1) continue;
    const chunk = world.chunks.get(key);
    // A grid made now holds nothing yet, and every cell has changed.
    if (!old) {
      for (const { pos, light: lit } of chunk.values()) {
        const i = at(pos[0], pos[1], pos[2]);
        cls[i] = lit;
        steps[i] = step[lit];
      }
      continue;
    }
    let n = 0;
    for (let y = y0; y <= y1; y++) for (let z = z0; z <= z1; z++) {
      const row = at(x0, y, z);
      for (let i = row; i <= row + x1 - x0; i++) {
        was[n++] = cls[i];
        cls[i] = 0;
        steps[i] = airStep;
      }
    }
    if (chunk) {
      for (const { pos, light: lit } of chunk.values()) {
        const i = at(pos[0], pos[1], pos[2]);
        cls[i] = lit;
        steps[i] = step[lit];
      }
    }
    // With every cell changed already, there is nothing to compare.
    if (count === vol) continue;
    n = 0;
    for (let y = y0; y <= y1; y++) for (let z = z0; z <= z1; z++) {
      const row = at(x0, y, z);
      for (let i = row; i <= row + x1 - x0; i++, n++) if (cls[i] !== was[n]) seed(i);
    }
  }
  // Whether the face shapes light classes `a` and `b` turn to each other
  // close, `a`'s face `from` against `b`'s opposite one, `to`.
  const closed = (a, b, from, to) => occludes[face[a * 6 + from] * shapes + face[b * 6 + to]] === 1;
  // Each read column's highest cell the sky does not reach (from air above
  // the box, down while each cell passes it and the faces between do not
  // close), and the span whose open sky it opened or closed.
  for (const column of columns) {
    let top = NO_TOP;
    // The class of the cell above where it shapes light (else air's, whose faces are empty too).
    let above = 0;
    let aboveStep = airStep;
    for (let y = maxY; y >= minY; y--) {
      const i = (y - minY) * plane + column;
      const here = steps[i];
      if (!(here & SKY) || ((aboveStep | here) & SHAPED && closed(above, cls[i], DOWN, UP))) { top = y; break; }
      above = here & SHAPED ? cls[i] : 0;
      aboveStep = here;
    }
    const before = g.top[column];
    if (before === top) continue;
    g.top[column] = top;
    const low = Math.max(Math.min(before, top), minY - 1);
    // Within the box: a highest cell above it left with blocks the box dropped.
    const high = Math.min(Math.max(before, top), maxY);
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
  // lit from its own sources. A cell under open sky gives light only to a
  // neighbour that is not: the cell under it if that is its column's highest
  // the sky does not reach, or one beside it in a column whose highest is as
  // high or higher. The rest are not queued, which leaves the field as it
  // is: most cells of a first look are sky over the build.
  const { top } = g;
  const opensOnto = (column, y) => y - 1 <= top[column]
    || (column % sx + 1 < sx && top[column + 1] >= y) || (column % sx > 0 && top[column - 1] >= y)
    || (column + sx < plane && top[column + sx] >= y) || (column >= sx && top[column - sx] >= y);
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
    const column = index % plane;
    if (y > top[column]) {
      g.lvl[index] = 15;
      if (opensOnto(column, y)) enqueue(index);
    } else {
      const lit = steps[index] & EMITS ? emission[cls[index]] : 0;
      g.lvl[index] = lit;
      if (lit > 0) enqueue(index);
    }
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
  // A step from a cell (class `from`, level `level`, its step `fromStep`)
  // into neighbour `to` across its face `out` (the neighbour's `back`). A
  // step costs at least 1, so a neighbour at level - 1 or more takes
  // nothing, which is known before its class is read: most cells of a lit
  // world are skylit beside skylit cells.
  const { lvl } = g;
  const spread = (from, level, fromStep, to, out, back) => {
    if (lvl[to] >= level - 1) return;
    const toStep = steps[to];
    const next = level - (toStep & 15);
    if (next <= lvl[to]) return;
    if ((fromStep | toStep) & SHAPED && closed(from, cls[to], out, back)) return;
    lvl[to] = next;
    enqueue(to);
  };
  while (pending > 0) {
    const index = dequeue();
    const level = lvl[index];
    if (level <= 1) continue;
    const fromStep = steps[index];
    const from = fromStep & SHAPED ? cls[index] : 0;
    const x = index % sx;
    const z = Math.floor(index / sx) % sz;
    if (x + 1 < sx) spread(from, level, fromStep, index + 1, EAST, WEST);
    if (x > 0) spread(from, level, fromStep, index - 1, WEST, EAST);
    if (index + plane < vol) spread(from, level, fromStep, index + plane, UP, DOWN);
    if (index >= plane) spread(from, level, fromStep, index - plane, DOWN, UP);
    if (z + 1 < sz) spread(from, level, fromStep, index + sx, SOUTH, NORTH);
    if (z > 0) spread(from, level, fromStep, index - sx, NORTH, SOUTH);
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
