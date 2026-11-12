'use strict';

const vm = require('vm');

const { World } = require('./world');
const { validateWorldHeight } = require('./minecraft-dimension');

const VALID_FILL_MODES = new Set(['replace', 'hollow', 'outline']);
const MAX_LOG_ENTRIES = 64;
const MAX_LOG_ENTRY_CHARS = 300;
const MAX_FIND_RESULTS = 500;
const SCRIPT_TIMEOUT_MS = 5000;
const MAX_WRITE_FILES = 16;
const MAX_WRITE_TOTAL = 32 * 1024 * 1024;
const FLOOR_TORCHES = new Set([
  'minecraft:torch',
  'minecraft:soul_torch',
  'minecraft:redstone_torch',
]);
const WALL_TORCHES = new Set([
  'minecraft:wall_torch',
  'minecraft:soul_wall_torch',
  'minecraft:redstone_wall_torch',
]);
const FACING_VECTOR = {
  north: [0, 0, -1],
  south: [0, 0, 1],
  east: [1, 0, 0],
  west: [-1, 0, 0],
};

function isPlainObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
    && Object.prototype.toString.call(value) === '[object Object]';
}

// blockRegistry: { name: [propertiesObject, defaultsObject] } from the mcmeta blocks summary.
// Validates ids and block states against the real game data, which is also
// exactly what the renderer can draw.
function createBlockNormalizer(blockRegistry) {
  function normalize(spec, label) {
    let name;
    let states = {};
    if (typeof spec === 'string') {
      name = spec;
      const bracket = spec.indexOf('[');
      if (bracket !== -1) {
        if (!spec.endsWith(']')) throw new Error(`${label}: malformed block states in "${spec}"`);
        name = spec.slice(0, bracket);
        for (const pair of spec.slice(bracket + 1, -1).split(',')) {
          if (!pair) continue;
          const eq = pair.indexOf('=');
          if (eq === -1) throw new Error(`${label}: malformed block state "${pair}" in "${spec}"`);
          states[pair.slice(0, eq).trim()] = pair.slice(eq + 1).trim();
        }
      }
    } else if (isPlainObject(spec) && typeof spec.id === 'string') {
      name = spec.id;
      if (spec.states != null) {
        if (!isPlainObject(spec.states)) throw new Error(`${label}: states must be an object`);
        states = spec.states;
      }
    } else {
      throw new Error(`${label}: block must be a string like "oak_planks" or "oak_stairs[facing=north]", or { id, states }`);
    }

    name = name.replace(/^minecraft:/, '').trim();
    if (!name) throw new Error(`${label}: empty block id`);
    if (name === 'air') return { id: 'minecraft:air', states: {} };

    const entry = blockRegistry[name];
    if (!entry) {
      const close = Object.keys(blockRegistry)
        .filter((k) => k.includes(name) || name.includes(k))
        .slice(0, 6);
      throw new Error(`${label}: unknown block "${name}"${close.length ? ` (closest matches: ${close.join(', ')})` : ''}`);
    }
    const [properties] = entry;

    const normalizedStates = {};
    for (const [k, v] of Object.entries(states)) {
      const allowed = properties[k];
      if (!allowed) {
        throw new Error(`${label}: block "${name}" has no state "${k}" (valid: ${Object.keys(properties).join(', ') || 'none'})`);
      }
      const value = String(v);
      if (!allowed.includes(value)) {
        throw new Error(`${label}: "${name}" state ${k}=${value} is invalid (valid: ${allowed.join(', ')})`);
      }
      normalizedStates[k] = value;
    }
    return { id: `minecraft:${name}`, states: normalizedStates };
  }
  return normalize;
}

// Changed chunks and their face neighbours: a block whose partner or support
// is one block away can only have been affected there.
function nearChangedChunks(world) {
  const keys = new Set();
  for (const key of world.changedChunks) {
    const [cx, cy, cz] = World.coordsOf(key);
    keys.add(key);
    keys.add(World.keyOf(cx + 1, cy, cz)); keys.add(World.keyOf(cx - 1, cy, cz));
    keys.add(World.keyOf(cx, cy + 1, cz)); keys.add(World.keyOf(cx, cy - 1, cz));
    keys.add(World.keyOf(cx, cy, cz + 1)); keys.add(World.keyOf(cx, cy, cz - 1));
  }
  return keys;
}

function validateTorchSupport(world, blockRegistry) {
  if (!world.palette.entries.some((entry) => entry
    && (FLOOR_TORCHES.has(entry.id) || WALL_TORCHES.has(entry.id)))) return;
  for (const key of nearChangedChunks(world)) {
    world.forEachInChunk(key, (x, y, z, block) => {
      let support;
      if (FLOOR_TORCHES.has(block.id)) {
        support = [x, y - 1, z];
      } else if (WALL_TORCHES.has(block.id)) {
        const name = block.id.slice('minecraft:'.length);
        const facing = block.states.facing || blockRegistry[name][1].facing;
        const outward = FACING_VECTOR[facing];
        support = [x - outward[0], y, z - outward[2]];
      } else {
        return;
      }
      if (!world.get(support[0], support[1], support[2])) {
        const kind = WALL_TORCHES.has(block.id) ? 'wall torch' : 'floor torch';
        throw new Error(
          `Unsupported ${kind} at [${x}, ${y}, ${z}]: ${block.id.replace('minecraft:', '')}`
          + ` needs a block at [${support.join(', ')}]`
        );
      }
    });
  }
}

// A bed is two blocks of one colour and facing: the head sits one block from
// the foot in the direction the bed faces. Minecraft keeps a half whose
// partner is missing or mismatched, and shows it as a broken bed.
function validateBeds(world, blockRegistry) {
  if (!world.palette.entries.some((entry) => entry?.id.endsWith('_bed'))) return;
  for (const key of nearChangedChunks(world)) {
    world.forEachInChunk(key, (x, y, z, block) => {
      if (!block.id.endsWith('_bed')) return;
      const name = block.id.slice('minecraft:'.length);
      const defaults = blockRegistry[name][1];
      const facing = block.states.facing || defaults.facing;
      const part = block.states.part || defaults.part;
      const partner = part === 'foot' ? 'head' : 'foot';
      const step = part === 'foot' ? 1 : -1;
      const at = [x + FACING_VECTOR[facing][0] * step, y, z + FACING_VECTOR[facing][2] * step];
      const other = world.get(at[0], at[1], at[2]);
      if (other?.id === block.id && (other.states.part || defaults.part) === partner && (other.states.facing || defaults.facing) === facing) return;
      throw new Error(
        `Incomplete bed at [${x}, ${y}, ${z}]: ${name}[part=${part},facing=${facing}] needs its ${partner} at [${at.join(', ')}]`
        + ` with the same facing (a bed faces from its foot toward its head)`
      );
    });
  }
}

function normVec3(raw, label) {
  if (!Array.isArray(raw) || raw.length !== 3) throw new Error(`${label} must be a [x, y, z] array`);
  return raw.map((value, idx) => {
    const n = Number(value);
    if (!Number.isInteger(n)) throw new Error(`${label}[${idx}] must be an integer`);
    return n;
  });
}

function normalizeAxisStep(raw, label) {
  const step = normVec3(raw, label);
  const nonZero = step.filter((v) => v !== 0);
  if (nonZero.length !== 1 || Math.abs(nonZero[0]) !== 1) {
    throw new Error(`${label} must be an axis-aligned unit vector like [1,0,0] or [0,0,-1]`);
  }
  return step;
}

function dot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function addScaled(base, step, times) {
  return [base[0] + step[0] * times, base[1] + step[1] * times, base[2] + step[2] * times];
}

function normalizeGridRows(rows, label) {
  if (!Array.isArray(rows) || rows.length === 0) throw new Error(`${label} must be a non-empty array`);
  const normalized = rows.map((row, i) => {
    if (typeof row === 'string') {
      if (!row.length) throw new Error(`${label}[${i}] must not be empty`);
      return Array.from(row);
    }
    if (!Array.isArray(row) || row.length === 0) throw new Error(`${label}[${i}] must be a non-empty string or array`);
    return row.map((cell, j) => {
      if (typeof cell !== 'string' || !cell.length) throw new Error(`${label}[${i}][${j}] must be a non-empty string`);
      return cell;
    });
  });
  const width = normalized[0].length;
  for (let i = 0; i < normalized.length; i += 1) {
    if (normalized[i].length !== width) {
      throw new Error(`${label}[${i}] has width ${normalized[i].length}; expected ${width}`);
    }
  }
  return normalized;
}

// Runs a build program against a clone of the world. On success the caller
// swaps the clone in; on failure the original world is untouched.
// `settled`, when given, is { world(): the World Minecraft made of the world as
// it was when the program started (shot-world.js), which mc.settled reads,
// ticks: the ticks it ran, which is mc.settledTicks }.
function createRuntime(world, blockRegistry, readFile, settled) {
  const normalizeBlock = createBlockNormalizer(blockRegistry);
  const offsetStack = [[0, 0, 0]];
  const logs = [];
  // Pending /work writes: buffered here and flushed by the caller only when
  // the whole program succeeds, matching the world's atomicity.
  const files = [];
  let filesBytes = 0;
  let blocksTouched = 0;

  function bufferFile(relPath, text, label) {
    if (typeof relPath !== 'string' || !relPath.trim()) throw new Error(`${label}: path must be a non-empty string`);
    if (files.length >= MAX_WRITE_FILES) throw new Error(`${label}: at most ${MAX_WRITE_FILES} files per program`);
    filesBytes += text.length;
    if (filesBytes > MAX_WRITE_TOTAL) throw new Error(`${label}: total written data exceeds ${MAX_WRITE_TOTAL} bytes`);
    files.push({ path: relPath, text });
  }

  const top = () => offsetStack[offsetStack.length - 1];

  function worldPos(raw, label) {
    const p = normVec3(raw, label);
    const o = top();
    return [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
  }

  function setBlock(x, y, z, normalized) {
    world.set(x, y, z, normalized.id, normalized.states);
    blocksTouched += 1;
  }

  function regionCorners(from, to, label) {
    const a = worldPos(from, `${label} from`);
    const b = worldPos(to, `${label} to`);
    return {
      min: [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.min(a[2], b[2])],
      max: [Math.max(a[0], b[0]), Math.max(a[1], b[1]), Math.max(a[2], b[2])],
    };
  }

  function fillRegion(min, max, normalized) {
    world.fill(min, max, normalized.id, normalized.states);
    blocksTouched += (max[0] - min[0] + 1) * (max[1] - min[1] + 1) * (max[2] - min[2] + 1);
  }

  function normalizeStructuredPalette(rawPalette, label) {
    if (!isPlainObject(rawPalette)) throw new Error(`${label} must be an object`);
    const map = new Map();
    for (const [symbol, entry] of Object.entries(rawPalette)) {
      if (!symbol) throw new Error(`${label} contains an empty symbol`);
      map.set(symbol, entry == null ? null : normalizeBlock(entry, `${label}.${symbol}`));
    }
    return map;
  }

  function normalizeEmptySymbols(raw, label) {
    if (raw == null) return new Set(['.']);
    const list = typeof raw === 'string' ? [raw] : raw;
    if (!Array.isArray(list) || list.length === 0) throw new Error(`${label} must be a non-empty string or array`);
    for (const entry of list) {
      if (typeof entry !== 'string' || !entry.length) throw new Error(`${label} entries must be non-empty strings`);
    }
    return new Set(list);
  }

  function placeGrid(base, rowStep, colStep, rows, paletteMap, emptySymbols, label) {
    for (let r = 0; r < rows.length; r += 1) {
      for (let c = 0; c < rows[r].length; c += 1) {
        const symbol = rows[r][c];
        if (emptySymbols.has(symbol)) continue;
        if (!paletteMap.has(symbol)) {
          throw new Error(`${label} palette is missing symbol ${JSON.stringify(symbol)} at row ${r}, col ${c}`);
        }
        const normalized = paletteMap.get(symbol);
        if (normalized == null) continue;
        const pos = addScaled(addScaled(base, rowStep, r), colStep, c);
        setBlock(pos[0], pos[1], pos[2], normalized);
      }
    }
  }

  function toLogString(value) {
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  const mc = {
    // The ticks Minecraft ran for mc.settled's blocks (null without a settle).
    settledTicks: settled ? settled.ticks : null,

    set(pos, block) {
      const p = worldPos(pos, 'mc.set pos');
      setBlock(p[0], p[1], p[2], normalizeBlock(block, 'mc.set'));
    },

    fill(from, to, block, mode = 'replace') {
      if (!VALID_FILL_MODES.has(mode)) {
        throw new Error(`mc.fill: invalid mode "${mode}" (valid: replace, hollow, outline)`);
      }
      const normalized = normalizeBlock(block, 'mc.fill');
      const { min, max } = regionCorners(from, to, 'mc.fill');
      if (mode === 'replace') {
        fillRegion(min, max, normalized);
        return;
      }
      // The shell and the interior are written as boxes that do not overlap,
      // so the block limit counts only what the call leaves. Per axis, each
      // of `ends` (the boundary layers, one when the box is one block thick),
      // `inner` (the layers between them) and `full` is a list of ranges;
      // boxes() takes one range per axis in every combination.
      const ends = min.map((value, i) => (value === max[i] ? [[value, value]] : [[value, value], [max[i], max[i]]]));
      const inner = min.map((value, i) => (max[i] - value > 1 ? [[value + 1, max[i] - 1]] : []));
      const full = min.map((value, i) => [[value, max[i]]]);
      const boxes = (lists) => lists.reduce(
        (out, ranges) => out.flatMap(([from, to]) => ranges.map(([a, b]) => [[...from, a], [...to, b]])),
        [[[], []]],
      );
      const [nx, ny, nz] = min.map((value, i) => max[i] - value + 1);
      blocksTouched += nx * ny * nz;
      if (mode === 'hollow') {
        // A box with no interior is all walls.
        if (inner.some((ranges) => !ranges.length)) {
          world.fill(min, max, normalized.id, normalized.states);
          return;
        }
        const walls = [
          ...boxes([ends[0], full[1], full[2]]),
          ...boxes([inner[0], ends[1], full[2]]),
          ...boxes([inner[0], inner[1], ends[2]]),
        ];
        world.fillBoxes(walls, normalized.id, normalized.states, boxes(inner));
        blocksTouched += (nx - 2) * (ny - 2) * (nz - 2);
        return;
      }
      // The edges are the cells on the boundary of at least two axes; every
      // other cell is cleared.
      const edges = [
        ...boxes([full[0], ends[1], ends[2]]),
        ...boxes([ends[0], inner[1], ends[2]]),
        ...boxes([ends[0], ends[1], inner[2]]),
      ];
      const interior = [
        ...boxes([full[0], inner[1], inner[2]]),
        ...boxes([inner[0], ends[1], inner[2]]),
        ...boxes([inner[0], inner[1], ends[2]]),
      ];
      world.fillBoxes(edges, normalized.id, normalized.states, interior);
      const ix = Math.max(0, nx - 2); const iy = Math.max(0, ny - 2); const iz = Math.max(0, nz - 2);
      blocksTouched += ix * iy * nz + ix * ny * iz + nx * iy * iz - 2 * ix * iy * iz;
    },

    clear(from, to) {
      const { min, max } = regionCorners(from, to, 'mc.clear');
      fillRegion(min, max, { id: 'minecraft:air', states: {} });
    },

    clearAll() {
      blocksTouched += world.size();
      world.clear();
    },

    get(pos) {
      const p = worldPos(pos, 'mc.get pos');
      world.checkCoord(p[0], p[1], p[2]);
      const entry = world.get(p[0], p[1], p[2]);
      if (!entry) return null;
      return { id: entry.id, states: { ...entry.states } };
    },

    // The block Minecraft made of a position of the world as it was when this
    // program started (the last settle), at mc.get's coordinates.
    settled(x, y, z) {
      if (!settled) throw new Error('mc.settled: no Minecraft settle for this program');
      if (![x, y, z].every(Number.isInteger)) throw new Error(`mc.settled takes three integers, mc.settled(x, y, z), not ${JSON.stringify([x, y, z])}`);
      const p = worldPos([x, y, z], 'mc.settled position');
      const settledWorld = settled.world();
      settledWorld.checkCoord(p[0], p[1], p[2]);
      const entry = settledWorld.get(p[0], p[1], p[2]);
      if (!entry) return null;
      return { id: entry.id, states: { ...entry.states } };
    },

    find(blockId, region) {
      const name = `minecraft:${String(blockId).replace(/^minecraft:/, '')}`;
      let box = null;
      if (region != null) {
        if (!isPlainObject(region) || !region.from || !region.to) {
          throw new Error('mc.find region must be { from: [x,y,z], to: [x,y,z] }');
        }
        box = regionCorners(region.from, region.to, 'mc.find region');
      }
      const results = [];
      world.forEach((x, y, z, v) => {
        if (v.id !== name) return;
        if (results.length >= MAX_FIND_RESULTS) {
          throw new Error(`mc.find: more than ${MAX_FIND_RESULTS} "${blockId}" matches — narrow with a region, or use mc.count/mc.replace/mc.dump for bulk work`);
        }
        results.push([x, y, z]);
      }, box);
      return results;
    },

    replace(matchId, block, region) {
      const name = `minecraft:${String(matchId).replace(/^minecraft:/, '')}`;
      let box = null;
      if (region != null) {
        if (!isPlainObject(region) || !region.from || !region.to) {
          throw new Error('mc.replace region must be { from: [x,y,z], to: [x,y,z] }');
        }
        box = regionCorners(region.from, region.to, 'mc.replace region');
      }
      const normalized = normalizeBlock(block, 'mc.replace');
      const isAir = normalized.id === 'minecraft:air';
      // States carry over where they still apply (oak_stairs -> spruce_stairs
      // keeps facing/half/shape); explicitly given states win.
      const validKeys = isAir ? {} : blockRegistry[normalized.id.replace(/^minecraft:/, '')][0];
      const matched = world.replace(name, (entry) => {
        if (isAir) return normalized;
        const states = { ...normalized.states };
        for (const [k, v] of Object.entries(entry.states)) {
          if (states[k] === undefined && validKeys[k] && validKeys[k].includes(v)) states[k] = v;
        }
        return { id: normalized.id, states };
      }, box);
      blocksTouched += matched;
      return matched;
    },

    count(blockId) {
      if (blockId == null) return world.size();
      const name = `minecraft:${String(blockId).replace(/^minecraft:/, '')}`;
      let n = 0;
      world.forEach((x, y, z, v) => { if (v.id === name) n += 1; });
      return n;
    },

    bounds() {
      return world.bounds();
    },

    grid2d(spec) {
      if (!isPlainObject(spec)) throw new Error('mc.grid2d spec must be an object');
      const base = worldPos(spec.origin || [0, 0, 0], 'mc.grid2d origin');
      // Screen convention: rows read downward from the origin, so ASCII art
      // renders exactly as written with origin at the top-left.
      const rowStep = normalizeAxisStep(spec.rowStep || [0, -1, 0], 'mc.grid2d rowStep');
      const colStep = normalizeAxisStep(spec.colStep || [0, 0, 1], 'mc.grid2d colStep');
      if (dot(rowStep, colStep) !== 0) throw new Error('mc.grid2d rowStep and colStep must be orthogonal');
      const rows = normalizeGridRows(spec.rows, 'mc.grid2d rows');
      const paletteMap = normalizeStructuredPalette(spec.palette || {}, 'mc.grid2d palette');
      const emptySymbols = normalizeEmptySymbols(spec.empty, 'mc.grid2d empty');
      placeGrid(base, rowStep, colStep, rows, paletteMap, emptySymbols, 'mc.grid2d');
    },

    layers(spec) {
      if (!isPlainObject(spec)) throw new Error('mc.layers spec must be an object');
      if (!Array.isArray(spec.layers) || spec.layers.length === 0) {
        throw new Error('mc.layers spec.layers must be a non-empty array');
      }
      const base = worldPos(spec.origin || [0, 0, 0], 'mc.layers origin');
      // Map convention: each text line runs west->east, successive lines go
      // north->south, so a layer reads like a floor plan with north at the top.
      const rowStep = normalizeAxisStep(spec.rowStep || [0, 0, 1], 'mc.layers rowStep');
      const colStep = normalizeAxisStep(spec.colStep || [1, 0, 0], 'mc.layers colStep');
      const layerStep = normalizeAxisStep(spec.layerStep || [0, 1, 0], 'mc.layers layerStep');
      if (dot(rowStep, colStep) !== 0 || dot(rowStep, layerStep) !== 0 || dot(colStep, layerStep) !== 0) {
        throw new Error('mc.layers rowStep, colStep, and layerStep must be mutually orthogonal');
      }
      const paletteMap = normalizeStructuredPalette(spec.palette || {}, 'mc.layers palette');
      const emptySymbols = normalizeEmptySymbols(spec.empty, 'mc.layers empty');
      for (let i = 0; i < spec.layers.length; i += 1) {
        const rawRows = isPlainObject(spec.layers[i]) ? spec.layers[i].rows : spec.layers[i];
        const rows = normalizeGridRows(rawRows, `mc.layers layers[${i}]`);
        placeGrid(addScaled(base, layerStep, i), rowStep, colStep, rows, paletteMap, emptySymbols, `mc.layers layers[${i}]`);
      }
    },

    translate(dx, dy, dz, fn) {
      if (typeof fn !== 'function') throw new Error('mc.translate requires a function');
      const [x, y, z] = normVec3([dx, dy, dz], 'mc.translate offset');
      const o = top();
      offsetStack.push([o[0] + x, o[1] + y, o[2] + z]);
      try {
        return fn();
      } finally {
        offsetStack.pop();
      }
    },

    spec(id, states) {
      return { id, states: states || {} };
    },

    // Reads a text file from the sandbox /work directory (written by the bash
    // tool). The bridge for data too large to pass through model context.
    readFile(relPath) {
      if (typeof readFile !== 'function') throw new Error('mc.readFile: no sandbox for this conversation');
      if (typeof relPath !== 'string' || !relPath.trim()) throw new Error('mc.readFile: path must be a non-empty string');
      return readFile(relPath);
    },

    // The reverse bridge: write a text file into /work for bash to analyze.
    // Buffered — files land only if the whole program succeeds.
    writeFile(relPath, text) {
      if (typeof text !== 'string') throw new Error('mc.writeFile: text must be a string (JSON.stringify your data)');
      bufferFile(relPath, text, 'mc.writeFile');
    },

    // Dumps blocks (optionally within a region) to /work as JSON:
    // { palette: [{id, states}], blocks: [[x, y, z, paletteIndex]] }.
    dump(relPath, region) {
      let box = null;
      if (region != null) {
        if (!isPlainObject(region) || !region.from || !region.to) {
          throw new Error('mc.dump region must be { from: [x,y,z], to: [x,y,z] }');
        }
        box = regionCorners(region.from, region.to, 'mc.dump region');
      }
      const palette = [];
      const paletteIndex = new Map();
      const blocks = [];
      world.forEach((x, y, z, v) => {
        let idx = paletteIndex.get(v);
        if (idx === undefined) {
          idx = palette.length;
          palette.push({ id: v.id, states: v.states });
          paletteIndex.set(v, idx);
        }
        blocks.push([x, y, z, idx]);
      }, box);
      bufferFile(relPath, JSON.stringify({ palette, blocks }), 'mc.dump');
      return blocks.length;
    },

    log(...args) {
      if (logs.length >= MAX_LOG_ENTRIES) return;
      let entry = args.map(toLogString).join(' ');
      if (entry.length > MAX_LOG_ENTRY_CHARS) {
        entry = `${entry.slice(0, MAX_LOG_ENTRY_CHARS)}...(truncated)`;
      }
      logs.push(entry);
      if (logs.length === MAX_LOG_ENTRIES) logs.push('(log limit reached)');
    },
  };

  return { mc, logs, files, touched: () => blocksTouched };
}

// node:vm is a fault barrier, not a security boundary — but the classic escape
// (reach the host realm's Function constructor through any host object handed
// into the context) IS closable. The rule: nothing on the context's globals
// may be a host-realm object. This bootstrap runs INSIDE the context and is
// the only code that ever touches the host `mc`; it captures it in a closure
// (unreachable from user code) and exposes an in-realm facade whose methods
// deep-clone returns and re-throw errors as in-realm Errors. So user code can
// only ever see context-realm objects, whose .constructor is the harmless
// context Function. Math/JSON are the context's own built-ins, and
// Math.random is reseeded in-realm from the program source (FNV-1a +
// mulberry32) so history replay stays deterministic.
const BOOTSTRAP = `(function (host, source) {
  'use strict';
  var G = globalThis;
  var clone = function (v) { return v === undefined ? undefined : JSON.parse(JSON.stringify(v)); };
  var mc = {};
  Object.keys(host).forEach(function (name) {
    var target = host[name];
    if (typeof target === 'number' || target === null) { mc[name] = target; return; }
    if (typeof target !== 'function') return;
    mc[name] = function () {
      try {
        return clone(target.apply(host, arguments));
      } catch (e) {
        throw new Error(e && e.message ? String(e.message) : String(e));
      }
    };
  });
  var h = 2166136261;
  for (var i = 0; i < source.length; i += 1) { h ^= source.charCodeAt(i); h = Math.imul(h, 16777619); }
  var a = h >>> 0;
  Math.random = function () {
    a = (a + 0x6d2b79f5) | 0;
    var t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  G.mc = mc;
  G.module = { exports: {} };
  G.exports = G.module.exports;
  G.console = { log: mc.log };
})`;

// Executes an LLM-written program. The program must assign a synchronous
// function to module.exports. Definition and invocation run in one vm script
// so the timeout bounds the entire execution.
function runProgram(source, world, blockRegistry, readFile, settled) {
  const runtime = createRuntime(world, blockRegistry, readFile, settled);
  const context = vm.createContext({});
  // Trusted bootstrap: hand the host mc + source in as arguments (they stay
  // closure-local, never reachable as context globals) and let it install the
  // in-realm facade. runInContext returns the bootstrap function itself.
  const makeMc = new vm.Script(BOOTSTRAP, { filename: 'bootstrap.js' }).runInContext(context);
  makeMc(runtime.mc, source);
  const wrapped = `${source}
;(function () {
  const fn = module.exports;
  if (typeof fn !== 'function') throw new Error('Program must assign a function to module.exports');
  const result = fn(mc);
  if (result && typeof result.then === 'function') {
    throw new Error('Program must be synchronous (do not use async/await)');
  }
})();`;
  try {
    new vm.Script(wrapped, { filename: 'build.js' }).runInContext(context, { timeout: SCRIPT_TIMEOUT_MS });
    validateWorldHeight(world);
    validateTorchSupport(world, blockRegistry);
    validateBeds(world, blockRegistry);
  } catch (err) {
    // Logs written before the failure are debugging gold for the retry; hang
    // them on the error since the world/file changes are discarded anyway.
    if (err && typeof err === 'object') err.logs = runtime.logs;
    throw err;
  }
  return { logs: runtime.logs, files: runtime.files, touched: runtime.touched() };
}

// Formats a program failure for the model: the message plus only the stack
// frames that point into the program's own source (build.js), including vm's
// source-line/caret excerpt when it does. Host and bootstrap frames are noise.
function formatProgramError(err) {
  if (!err || !err.stack) return String(err);
  const lines = String(err.stack).split('\n');
  const firstFrame = lines.findIndex((line) => /^\s+at /.test(line));
  const head = firstFrame === -1 ? lines : lines.slice(0, firstFrame);
  const frames = firstFrame === -1 ? [] : lines.slice(firstFrame).filter((line) => line.includes('build.js:'));
  const kept = /^build\.js:\d+/.test(head[0]) ? head : head.slice(-1);
  return [...kept, ...frames].join('\n').trim();
}

module.exports = { runProgram, formatProgramError, createBlockNormalizer };
