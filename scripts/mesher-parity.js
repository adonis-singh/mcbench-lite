'use strict';

// The mesher (renderer/) against a reference build of itself from git, on
// real worlds: every tile of every world drawn by both, byte for byte, and
// what every load and patch answers (renderer/parity/src/main.rs).
//
//   node scripts/mesher-parity.js [--ref <git ref>] [--ticks n] [--random n] [--out folder] [blueprint ...]
//
// The worlds: each community build of the corpus MCBENCH_CORPUS names
// (test/helpers/community.js; none while it is unset) as the page's
// engine places it (scripts/wasm-node-engine.js), with the patches of its
// ticks (levers toggled, buttons pressed, as scripts/core-parity.js does);
// worlds the engine grows and shrinks as it ticks them (water falling off a
// ledge to the world's floor and drying up when its source goes, pistons
// pushing blocks into the next chunk columns and pulling them back, sand
// falling off a tower, lava running down a wall); built-up scenes for light
// (sources, roofs over deep gaps, caves, water, leaves, slabs, a tower 900
// blocks tall, blocks at the world's floor and ceiling); builds side by side
// with empty chunks between, as a bench lays them out; and any blueprint
// files given. Needs the baked pack of the data directory (MCBENCH_DATA_DIR)
// and cargo.

const fs = require('fs');
const os = require('os');
const path = require('path');
const readline = require('readline');
const { execFileSync, spawn } = require('child_process');

const { World } = require('../server/world');
const { importCommands } = require('../server/live-placement');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { build } = require('./build-wasm');
const { startNodeEngine } = require('./wasm-node-engine');
const { communityBuilds, communityBlocks, bakedPack } = require('../test/helpers/community');

const ROOT = path.join(__dirname, '..');
const RENDERER = path.join(ROOT, 'renderer');
const REFERENCE = path.join(RENDERER, 'target', 'reference');

function option(name, fallback) {
  const at = process.argv.indexOf(name);
  if (at < 0) return fallback;
  const value = process.argv[at + 1];
  process.argv.splice(at, 2);
  return value;
}

// The reference's sources, as a crate of its own name.
function extractReference(ref) {
  fs.rmSync(REFERENCE, { recursive: true, force: true });
  const files = execFileSync('git', ['ls-tree', '-r', '--name-only', ref, 'renderer/src', 'renderer/Cargo.toml'], { cwd: ROOT, encoding: 'utf8' }).split('\n').filter(Boolean);
  for (const file of files) {
    const target = path.join(REFERENCE, path.relative('renderer', file));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, execFileSync('git', ['show', `${ref}:${file}`], { cwd: ROOT, maxBuffer: 1 << 28 }));
  }
  const manifest = path.join(REFERENCE, 'Cargo.toml');
  fs.writeFileSync(manifest, fs.readFileSync(manifest, 'utf8').replace('name = "mcbench-renderer"', 'name = "mcbench-renderer-reference"'));
}

function writeWorld(folder, name, steps, bounds) {
  const dir = path.join(folder, name);
  fs.mkdirSync(dir, { recursive: true });
  const mask = renderMask(bounds);
  fs.writeFileSync(path.join(dir, 'mask'), `${mask.minX} ${mask.maxX} ${mask.minZ} ${mask.maxZ}\n`);
  steps.forEach(({ kind, bytes }, n) => fs.writeFileSync(path.join(dir, `${String(n).padStart(5, '0')}.${kind}`), bytes));
}

// Blocks ("x,y,z" -> state, Minecraft heights) to bounds as the views give them.
function boundsOf(blocks) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const key of blocks.keys()) {
    key.split(',').map(Number).forEach((value, axis) => { min[axis] = Math.min(min[axis], value); max[axis] = Math.max(max[axis], value); });
  }
  return { min, max };
}

const blueprintOf = (blocks) => blocksBlueprint([...blocks].map(([key, name]) => [...key.split(',').map(Number), name]));

// A community build as the page's engine places and ticks it: steps of
// blueprints and patches.
async function engineSteps(engine, blocks, ticks) {
  const world = new World();
  for (const [key, name] of blocks) {
    const [x, y, z] = key.split(',').map(Number);
    const open = name.indexOf('[');
    const states = open < 0 ? {} : Object.fromEntries(name.slice(open + 1, -1).split(',').map((pair) => pair.split('=')));
    world.set(x, y - Y_OFFSET, z, open < 0 ? name : name.slice(0, open), states);
  }
  world.finalize();
  const steps = [{ kind: 'blueprint', bytes: await engine.place(importCommands(world, null, Y_OFFSET).commands.map((line) => `${line}\n`).join('')) }];
  const inputs = [...blocks].filter(([, name]) => /^minecraft:lever\b|_button\b/.test(name));
  for (let t = 1; t <= ticks; t += 1) {
    for (const [key, name] of inputs) {
      const [x, y, z] = key.split(',').map(Number);
      if (name.startsWith('minecraft:lever') && (t === 2 || t === Math.floor(ticks / 2))) await engine.input('toggle', x, y, z).catch(() => {});
      if (/_button\b/.test(name) && t === 4) await engine.input('press', x, y, z).catch(() => {});
    }
    steps.push({ kind: 'patch', bytes: (await engine.tick(1)).bytes });
  }
  return steps;
}

// Worlds that grow and shrink as the engine ticks them, at Minecraft
// heights: each placed, ticked, then edited on the world as it is (a new
// base) and ticked on. `area` is what the render mask covers.
const GROWTH = {
  // Water off a ledge falls to the world's floor and spreads there; without
  // its source the fall dries up from the top.
  'falling-water': {
    place: ['fill 0 -40 0 15 -40 3 minecraft:stone', 'setblock 4 -34 6 minecraft:stone', 'setblock 4 -33 6 minecraft:water', 'fill 20 70 20 24 70 24 minecraft:stone', 'setblock 22 71 26 minecraft:water'],
    ticks: 260, edit: ['setblock 4 -33 6 minecraft:air', 'setblock 22 71 26 minecraft:air'], after: 320,
    area: { min: [-20, -64, -20], max: [50, 80, 50] },
  },
  // Pistons on a clock push blocks across chunk borders and pull them back.
  pistons: {
    place: [
      'fill 0 64 0 15 64 15 minecraft:stone',
      'setblock 13 65 2 minecraft:redstone_block', 'setblock 14 65 2 minecraft:sticky_piston[facing=east,extended=false]', 'setblock 15 65 2 minecraft:stone',
      'setblock 2 65 1 minecraft:redstone_block', 'setblock 2 65 0 minecraft:sticky_piston[facing=north,extended=false]', 'setblock 2 65 -1 minecraft:glowstone',
      'setblock 8 65 8 minecraft:redstone_block', 'setblock 8 66 8 minecraft:piston[facing=up,extended=false]', 'setblock 8 67 8 minecraft:slime_block',
    ],
    ticks: 40, edit: ['setblock 13 65 2 minecraft:air', 'setblock 2 65 1 minecraft:air', 'setblock 8 65 8 minecraft:air'], after: 40,
    area: { min: [-20, 60, -20], max: [40, 80, 40] },
  },
  // Sand and gravel off a tower's top, over its edge into the next column.
  'falling-sand': {
    place: ['fill 0 64 0 3 90 3 minecraft:stone', 'fill 0 91 0 3 91 3 minecraft:sand', 'fill 4 91 0 6 93 2 minecraft:gravel', 'setblock 4 90 0 minecraft:stone'],
    ticks: 120, edit: ['fill 0 64 0 3 64 3 minecraft:air'], after: 60,
    area: { min: [-20, 60, -20], max: [30, 100, 30] },
  },
  // Lava down a wall and over the floor's edge.
  lava: {
    place: ['fill 0 64 0 15 64 15 minecraft:stone', 'fill 0 65 0 0 75 15 minecraft:stone', 'setblock 1 75 5 minecraft:lava'],
    ticks: 300, edit: ['setblock 1 75 5 minecraft:air'], after: 300,
    area: { min: [-20, 40, -20], max: [40, 80, 40] },
  },
};

async function growthSteps(engine, { place, ticks, edit, after }) {
  const commands = (lines) => lines.map((line) => `${line}\n`).join('');
  const steps = [{ kind: 'blueprint', bytes: await engine.place(commands(place)) }];
  for (let t = 0; t < ticks; t += 1) steps.push({ kind: 'patch', bytes: (await engine.tick(1)).bytes });
  steps.push({ kind: 'blueprint', bytes: await engine.place(commands(edit), { fresh: false }) });
  for (let t = 0; t < after; t += 1) steps.push({ kind: 'patch', bytes: (await engine.tick(1)).bytes });
  return steps;
}

// Scenes for light, at Minecraft heights.
function scenes() {
  const out = {};
  const put = (blocks, x, y, z, name) => blocks.set(`${x},${y},${z}`, name);
  const box = (blocks, [x0, y0, z0], [x1, y1, z1], name) => {
    for (let x = x0; x <= x1; x += 1) for (let y = y0; y <= y1; y += 1) for (let z = z0; z <= z1; z += 1) put(blocks, x, y, z, name);
  };
  let seed = 12345;
  const next = () => { seed = (seed * 1103515245 + 12345) >>> 0; return seed / 2 ** 32; };

  // A room with holes in its roof, lights, water, leaves and slabs.
  const room = new Map();
  box(room, [0, 64, 0], [39, 64, 39], 'minecraft:stone');
  box(room, [0, 65, 0], [0, 72, 39], 'minecraft:oak_planks');
  box(room, [39, 65, 0], [39, 72, 39], 'minecraft:glass');
  box(room, [0, 73, 0], [39, 73, 39], 'minecraft:oak_planks');
  for (let n = 0; n < 30; n += 1) room.delete(`${1 + Math.floor(next() * 38)},73,${1 + Math.floor(next() * 38)}`);
  for (const [x, z, name] of [[5, 5, 'minecraft:glowstone'], [20, 8, 'minecraft:torch'], [30, 30, 'minecraft:lantern'], [12, 25, 'minecraft:sea_lantern'], [25, 18, 'minecraft:redstone_lamp[lit=true]'], [8, 33, 'minecraft:jack_o_lantern']]) put(room, x, 65, z, name);
  box(room, [15, 64, 15], [22, 64, 22], 'minecraft:water');
  box(room, [26, 70, 4], [34, 72, 12], 'minecraft:oak_leaves[distance=1,persistent=true,waterlogged=false]');
  box(room, [4, 68, 20], [12, 68, 28], 'minecraft:oak_slab[type=bottom,waterlogged=false]');
  box(room, [2, 66, 2], [3, 66, 3], 'minecraft:oak_slab[type=top,waterlogged=false]');
  out.room = room;

  // A hill of stone with tunnels lit by torches and lava.
  const cave = new Map();
  box(cave, [0, 40, 0], [31, 70, 31], 'minecraft:stone');
  for (let x = 2; x < 30; x += 1) { box(cave, [x, 50, 14], [x, 52, 16], 'minecraft:air'); }
  for (let z = 2; z < 30; z += 1) { box(cave, [14, 55, z], [16, 57, z], 'minecraft:air'); }
  box(cave, [14, 52, 14], [16, 55, 16], 'minecraft:air');
  for (const key of [...cave.keys()]) if (cave.get(key) === 'minecraft:air') cave.delete(key);
  put(cave, 5, 50, 15, 'minecraft:torch');
  put(cave, 25, 50, 15, 'minecraft:lava');
  put(cave, 15, 55, 4, 'minecraft:glowstone');
  for (let x = 0; x < 32; x += 1) cave.delete(`${x},70,${Math.floor(next() * 32)}`);
  out.cave = cave;

  // A tower 900 blocks tall with a roof at its top, a slab floating halfway
  // and a ground floor: deep dark gaps under the overhangs.
  const tall = new Map();
  box(tall, [0, -60, 0], [47, -60, 47], 'minecraft:grass_block[snowy=false]');
  box(tall, [20, -59, 20], [23, 899, 23], 'minecraft:stone_bricks');
  box(tall, [10, 900, 10], [33, 900, 33], 'minecraft:stone_bricks');
  box(tall, [0, 480, 30], [40, 480, 47], 'minecraft:smooth_stone_slab[type=bottom,waterlogged=false]');
  put(tall, 21, 899, 21, 'minecraft:glowstone');
  put(tall, 5, 470, 40, 'minecraft:sea_lantern');
  put(tall, 30, -59, 30, 'minecraft:torch');
  out.tall = tall;

  // A roof 60 wide over a lit floor 130 blocks below it.
  const roof = new Map();
  box(roof, [0, 64, 0], [79, 64, 79], 'minecraft:stone');
  box(roof, [10, 200, 10], [69, 200, 69], 'minecraft:oak_planks');
  for (let n = 0; n < 12; n += 1) put(roof, 10 + Math.floor(next() * 60), 65, 10 + Math.floor(next() * 60), 'minecraft:glowstone');
  roof.delete('40,200,40');
  out.roof = roof;

  // Blocks at the floor and ceiling of the world.
  const edges = new Map();
  box(edges, [0, -64, 0], [20, -64, 20], 'minecraft:bedrock');
  box(edges, [5, -63, 5], [8, -60, 8], 'minecraft:glass');
  put(edges, 10, -63, 10, 'minecraft:glowstone');
  box(edges, [0, 955, 0], [12, 955, 12], 'minecraft:stone');
  put(edges, 3, 954, 3, 'minecraft:torch');
  put(edges, 30, 319, 30, 'minecraft:glowstone');
  out.edges = edges;
  return out;
}

// Builds side by side along x with empty chunks between them, as a bench
// lays them out (server/bench-world.js).
function sideBySide(builds) {
  const out = new Map();
  let cursor = 0;
  for (const blocks of builds) {
    const { min, max } = boundsOf(blocks);
    const dx = cursor - (min[0] >> 4) * 16;
    const dz = -Math.round((min[2] + max[2]) / 2);
    for (const [key, name] of blocks) {
      const [x, y, z] = key.split(',').map(Number);
      out.set(`${x + dx},${y},${z + dz}`, name);
    }
    cursor += ((max[0] >> 4) - (min[0] >> 4) + 1 + 3) * 16;
  }
  return out;
}

async function main() {
  const ref = option('--ref', 'main');
  const ticks = Number(option('--ticks', 40));
  const randoms = option('--random', '40');
  const out = path.resolve(option('--out', fs.mkdtempSync(path.join(os.tmpdir(), 'mesher-parity-'))));
  const extra = process.argv.slice(2);
  const pack = bakedPack();
  if (!pack) throw new Error('No baked pack in the data directory');
  extractReference(ref);
  const folder = path.join(out, 'worlds');
  fs.rmSync(folder, { recursive: true, force: true });

  for (const [name, blocks] of Object.entries(scenes())) writeWorld(folder, `scene-${name}`, [{ kind: 'blueprint', bytes: blueprintOf(blocks) }], boundsOf(blocks));

  const builds = communityBuilds();
  const engine = await startNodeEngine(await build());
  for (const [name, scene] of Object.entries(GROWTH)) writeWorld(folder, `growth-${name}`, await growthSteps(engine, scene), scene.area);
  let placed = 0;
  for (const [n, { file }] of builds.entries()) {
    const name = `community-${String(n).padStart(3, '0')}-${path.basename(path.dirname(file))}`;
    const blocks = communityBlocks(file);
    let steps;
    try {
      steps = await engineSteps(engine, blocks, ticks);
      placed += 1;
    } catch (error) {
      // A build the page's engine refuses: its blocks as placed.
      steps = [{ kind: 'blueprint', bytes: blueprintOf(blocks) }];
    }
    writeWorld(folder, name, steps, boundsOf(blocks));
  }
  engine.terminate();
  for (let n = 0; n + 3 <= Math.min(builds.length, 12); n += 3) {
    const blocks = sideBySide(builds.slice(n, n + 3).map(({ file }) => communityBlocks(file)));
    writeWorld(folder, `bench-like-${n / 3}`, [{ kind: 'blueprint', bytes: blueprintOf(blocks) }], boundsOf(blocks));
  }
  for (const file of extra) {
    const bytes = fs.readFileSync(file);
    // The blueprint's bounds follow its palette.
    let at = 0;
    const int = () => { at += 4; return bytes.readInt32BE(at - 4); };
    const utf = () => { at += 2 + bytes.readUInt16BE(at); };
    for (let entries = int(); entries > 0; entries -= 1) {
      utf();
      for (let pairs = int(); pairs > 0; pairs -= 1) { utf(); utf(); }
    }
    const bounds = Array.from({ length: 6 }, int);
    writeWorld(folder, `file-${path.basename(file, path.extname(file))}`, [{ kind: 'blueprint', bytes }], { min: bounds.slice(0, 3), max: bounds.slice(3) });
  }
  console.log(`${builds.length} community builds (${placed} placed and ticked by the engine), worlds in ${folder}`);

  const packFile = path.join(out, 'pack.bin');
  fs.writeFileSync(packFile, pack);
  // The pair program's lines, passed on as they come. Its line on stdout ends
  // a world; a difference goes to stderr first as "<world>: ...", and a world
  // neither side could load is "<world>: no world loaded" (its differences
  // left out of the program's total, so its exit code alone misses them).
  const worlds = new Set(fs.readdirSync(folder));
  const ended = new Map();
  const differing = new Set();
  const nameOf = (line) => line.slice(0, line.indexOf(': '));
  const child = spawn('cargo', ['run', '--release', '--quiet', '--', packFile, folder, randoms], { cwd: path.join(RENDERER, 'parity'), stdio: ['ignore', 'pipe', 'pipe'] });
  const exited = new Promise((resolve) => child.on('close', resolve));
  readline.createInterface({ input: child.stderr }).on('line', (line) => {
    console.error(line);
    if (worlds.has(nameOf(line))) differing.add(nameOf(line));
  });
  readline.createInterface({ input: child.stdout }).on('line', (line) => {
    console.log(line);
    if (worlds.has(nameOf(line))) ended.set(nameOf(line), line.endsWith(': no world loaded') ? 'refused' : 'compared');
  });
  const code = await exited;
  // A world the program did not end (it panicked, or never ran) errored.
  const totals = { matched: 0, differed: 0, refused: 0, errored: 0 };
  for (const name of worlds) {
    if (!ended.has(name)) totals.errored += 1;
    else if (differing.has(name)) totals.differed += 1;
    else totals[ended.get(name) === 'refused' ? 'refused' : 'matched'] += 1;
  }
  console.log(`${totals.matched} matched, ${totals.differed} differed, ${totals.refused} refused by both, ${totals.errored} errored (the pair program exited ${code})`);
  // A run that compared nothing proves nothing.
  return code === 0 && !totals.differed && !totals.errored && totals.matched + totals.differed > 0;
}

main().then((passed) => process.exit(passed ? 0 : 1), (error) => {
  console.error(error.stderr ? String(error.stderr) : error);
  process.exit(1);
});
