'use strict';

// Checks the browser's mesher (renderer/, as web/mesher.mjs runs it) against
// BlueMap. For each community build in the corpus MCBENCH_CORPUS names (or those
// --match names), and for fixtures of BlueMap's riskiest cases, it places
// the build with the engine, draws the blueprint with BlueMap's own renderer
// (McbenchBlueMapReference.java), meshes the same blueprint with the
// WebAssembly module, and compares every tile byte for byte. It then edits
// each blueprint (a roof, a light, removed blocks), draws the edit with
// BlueMap afresh, and checks that the tiles the module reports changed,
// drawn again over the unchanged ones it keeps, are BlueMap's tiles of the
// edit. A differing tile is decoded and its first differing face printed,
// with the block it belongs to.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/render-parity.js [--limit N] [--match text]
//     [--no-builds] [--no-fixtures] [--keep folder] [--report file.json]
//
// It builds renderer/ (scripts/build-renderer.js, cargo on PATH with the
// wasm32-unknown-unknown target) and bakes the pack (McbenchBake.java) into
// a folder of its own beside the app's BlueMap folder. Run it against a
// scratch data folder, e.g. a clone made with
// `cp -c -R data/minecraft-live tmp/render-data/`: the engine and BlueMap
// write there. --keep saves both tile sets of each case whose tiles differ.

const fs = require('fs');
const path = require('path');
const readline = require('readline');
const { execFileSync, spawn } = require('child_process');

const ROOT = path.join(__dirname, '..');
if (!process.env.MCBENCH_DATA_DIR || path.resolve(process.env.MCBENCH_DATA_DIR) === path.join(ROOT, 'data')) {
  throw new Error('Set MCBENCH_DATA_DIR to a scratch copy of the data folder');
}

const { minecraftLive } = require('../server/minecraft-live');
const { blueMap, configFiles, renderMask } = require('../server/bluemap');
const { blocksBlueprint, readBlueprint, worldBlueprint } = require('../server/blueprint');
const { MC_VERSION } = require('../server/assets');
const { World } = require('../server/world');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { build, MODULE } = require('./build-renderer');
const { corpusArtifacts } = require('./corpus');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};
const flag = (name) => process.argv.includes(`--${name}`);
const RENDERER = path.join(ROOT, 'renderer');
const FOLDER = path.join(path.dirname(blueMap.root), 'render-parity');

function writeFiles(folder, files) {
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(folder, name)), { recursive: true });
    fs.writeFileSync(path.join(folder, name), text);
  }
}

// The baked pack, made as the app makes it, and BlueMap's flow angle for
// every water level and neighbourhood, which the native mesher is checked
// against. Then BlueMap's own renderer, started once, drawing blueprints.
async function bake(blockTable) {
  await blueMap.ensureJar();
  fs.rmSync(FOLDER, { recursive: true, force: true });
  writeFiles(FOLDER, configFiles(path.join(blueMap.root, 'data')));
  const classes = path.join(FOLDER, 'classes');
  execFileSync(process.env.MCBENCH_JAVAC || 'javac', ['-proc:none', '-classpath', blueMap.jar, '-d', classes,
    path.join(ROOT, 'server', 'McbenchBake.java'), path.join(__dirname, 'McbenchBlueMapReference.java')], { stdio: 'inherit' });
  const classpath = [blueMap.jar, classes].join(path.delimiter);
  const flowTable = path.join(FOLDER, 'flow-angles.bin');
  const started = Date.now();
  execFileSync('java', ['-Xmx2G', '-cp', classpath, 'McbenchBake', MC_VERSION, blockTable, flowTable],
    { cwd: FOLDER, stdio: ['ignore', 'inherit', 'inherit'] });
  console.log(`baked in ${Date.now() - started} ms`);
  execFileSync(path.join(RENDERER, 'target', 'release', 'mcbench-mesh'), ['flow', flowTable], { stdio: 'inherit' });

  const java = spawn('java', ['-Xmx4G', '-cp', classpath, 'McbenchBlueMapReference', MC_VERSION, blockTable], { cwd: FOLDER, stdio: ['pipe', 'pipe', 'inherit'] });
  const replies = readline.createInterface({ input: java.stdout })[Symbol.asyncIterator]();
  let drawn = 0;
  // Every tile BlueMap draws for `blueprint` (bytes) under `mask`, as
  // "x<X>/z<Z>.prbm" -> bytes.
  const render = async (blueprint, mask) => {
    const name = `case-${drawn += 1}`;
    fs.writeFileSync(path.join(FOLDER, `${name}.bin`), blueprint);
    writeFiles(path.join(FOLDER, `${name}-config`), configFiles(path.join(blueMap.root, 'data'), mask));
    java.stdin.write(`${name}.bin ${name}-config/config ${name}-tiles\n`);
    for (;;) {
      const { value, done } = await replies.next();
      if (done) throw new Error('BlueMap stopped');
      if (value.startsWith('MCBENCH_RENDERED ')) break;
    }
    const tiles = readTiles(path.join(FOLDER, `${name}-tiles`, 'live', 'tiles', '0'));
    for (const suffix of ['.bin', '-config', '-tiles']) fs.rmSync(path.join(FOLDER, `${name}${suffix}`), { recursive: true, force: true });
    return tiles;
  };
  return { pack: fs.readFileSync(path.join(FOLDER, 'web', 'mcbench-pack.bin')), render, stop: () => java.stdin.end() };
}

// The module as the viewer's worker runs it: every tile of a blueprint, as
// BlueMap's file storage names them, and `edit`, which loads an edited
// blueprint after it and draws only the tiles it reports changed.
async function loadMesher(pack) {
  const { Mesher } = await import('../web/mesher.mjs');
  const mesher = await Mesher.create(fs.readFileSync(MODULE), pack);
  const draw = (tiles) => new Map(tiles.map(([x, z]) => [`x${x}/z${z}.prbm`, Buffer.from(mesher.draw(x, z))]));
  return {
    mesh: (blueprint, mask) => draw(mesher.load(blueprint, mask, false).tiles),
    edit: (blueprint, mask, before) => {
      const { tiles, changed } = mesher.load(blueprint, mask, true);
      const redrawn = draw(changed);
      return { changed: changed.length, tiles: new Map(tiles.map(([x, z]) => `x${x}/z${z}.prbm`).map((key) => [key, redrawn.get(key) || before.get(key)])) };
    },
  };
}

function readTiles(folder) {
  const tiles = new Map();
  if (!fs.existsSync(folder)) return tiles;
  for (const x of fs.readdirSync(folder)) {
    for (const z of fs.readdirSync(path.join(folder, x))) tiles.set(`${x}/${z}`, fs.readFileSync(path.join(folder, x, z)));
  }
  return tiles;
}

// The edit: a 5x5 stone roof three blocks over the build's middle block, a
// glowstone beside it, and every 97th block removed.
function edited(states) {
  const keys = [...states.keys()].sort();
  const [cx, , cz] = keys[Math.floor(keys.length / 2)].split(',').map(Number);
  const top = Math.max(...keys.map((key) => key.split(',').map(Number)).filter(([x, , z]) => Math.abs(x - cx) <= 4 && Math.abs(z - cz) <= 4).map(([, y]) => y));
  const next = new Map(states);
  keys.forEach((key, i) => { if (i % 97 === 0) next.delete(key); });
  for (let dx = -2; dx <= 2; dx += 1) for (let dz = -2; dz <= 2; dz += 1) next.set(`${cx + dx},${top + 3},${cz + dz}`, 'minecraft:stone');
  next.set(`${cx + 4},${top + 1},${cz}`, 'minecraft:glowstone');
  return blocksBlueprint([...next].map(([key, name]) => [...key.split(',').map(Number), name]));
}

// A .prbm tile decoded: per attribute its per-vertex byte rows, and the
// material groups.
const ATTRIBUTES = ['position', 'normal', 'color', 'uv', 'ao', 'blocklight', 'sunlight'];
function decodePrbm(bytes) {
  const vertices = bytes[2] | bytes[3] << 8 | bytes[4] << 16;
  let at = 8;
  const attributes = {};
  for (let a = 0; a < 7; a += 1) {
    const start = at;
    while (bytes[at] !== 0) at += 1;
    const name = bytes.toString('ascii', start, at);
    const attributeFlags = bytes[at + 1];
    at = Math.ceil((at + 2) / 4) * 4;
    const stride = ((attributeFlags >> 4 & 3) + 1) * [0, 4, 0, 1, 2, 0, 4, 1, 2, 0, 4][attributeFlags & 15];
    attributes[name] = { flags: attributeFlags, stride, data: bytes.subarray(at, at + stride * vertices) };
    at += stride * vertices;
  }
  at = Math.ceil(at / 4) * 4;
  const groups = [];
  while (bytes.readInt32LE(at) !== -1) {
    groups.push({ material: bytes.readInt32LE(at), start: bytes.readInt32LE(at + 4), count: bytes.readInt32LE(at + 8) });
    at += 12;
  }
  return { vertices, attributes, groups, end: at + 4 };
}

function faceValues(tile, face) {
  const values = {};
  for (const name of ATTRIBUTES) {
    const { flags, stride, data } = tile.attributes[name];
    const rows = [];
    for (let v = face * 3; v < face * 3 + 3; v += 1) {
      const row = data.subarray(v * stride, (v + 1) * stride);
      const encoding = flags & 15;
      const read = encoding === 1 ? (i) => row.readFloatLE(i * 4) : encoding === 3 ? (i) => row.readInt8(i) : (i) => row[i];
      rows.push(Array.from({ length: stride / (encoding === 1 ? 4 : 1) }, (_, i) => read(i)));
    }
    values[name] = rows.map((row) => row.join(',')).join(' | ');
  }
  const group = tile.groups.find((g) => face * 3 >= g.start && face * 3 < g.start + g.count);
  values.material = group ? group.material : null;
  return values;
}

// Where a tile's first differing face lies, and what each side drew there.
function describeDifference(key, expected, actual, blueprint) {
  const java = decodePrbm(expected);
  const wasm = decodePrbm(actual);
  const lines = [`    ${key}: BlueMap ${java.vertices / 3} faces, ${java.groups.length} groups; wasm ${wasm.vertices / 3} faces, ${wasm.groups.length} groups`];
  const faces = Math.min(java.vertices, wasm.vertices) / 3;
  let first = -1;
  for (let face = 0; face < faces && first < 0; face += 1) {
    for (const name of ATTRIBUTES) {
      const { stride } = java.attributes[name];
      if (!java.attributes[name].data.subarray(face * 3 * stride, (face + 1) * 3 * stride)
        .equals(wasm.attributes[name].data.subarray(face * 3 * stride, (face + 1) * 3 * stride))) { first = face; break; }
    }
  }
  if (first < 0 && java.vertices === wasm.vertices) {
    const groupsSame = JSON.stringify(java.groups) === JSON.stringify(wasm.groups);
    lines.push(`      every face is identical; ${groupsSame ? 'groups identical, bytes differ elsewhere' : 'material groups differ'}`);
    return lines.join('\n');
  }
  if (first < 0) first = faces;
  const [, tx, tz] = key.match(/^x(-?\d+)\/z(-?\d+)\.prbm$/).map(Number);
  for (const [label, tile] of [['BlueMap', java], ['wasm', wasm]]) {
    if (first * 3 >= tile.vertices) { lines.push(`      ${label}: no face ${first}`); continue; }
    const values = faceValues(tile, first);
    const position = tile.attributes.position.data;
    const [px, py, pz] = [0, 1, 2].map((i) => position.readFloatLE(first * 36 + i * 4));
    const block = [tx * 32 + 2 + Math.floor(px), Math.floor(py), tz * 32 + 2 + Math.floor(pz)];
    lines.push(`      ${label} face ${first} (near ${block.join(',')}: ${blueprint.get(block.join(',')) || 'air'})`);
    for (const [name, value] of Object.entries(values)) lines.push(`        ${name}: ${value}`);
  }
  return lines.join('\n');
}

// Fixtures: worlds that exercise what the community builds may not.
const COLORS = ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black'];
const FACINGS = ['north', 'south', 'west', 'east'];
const WOODS = ['oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark_oak', 'mangrove', 'cherry', 'pale_oak', 'bamboo', 'crimson', 'warped'];

function grid(world, items, { x0 = 0, z0 = 0, y = 1, spacing = 2, width = 16, floor = 'minecraft:stone' } = {}) {
  items.forEach(([id, states], i) => {
    const x = x0 + (i % width) * spacing;
    const z = z0 + Math.floor(i / width) * spacing;
    if (floor) world.set(x, y - 1, z, floor);
    world.set(x, y, z, id, states || {});
  });
}

// A deterministic pseudo-random sequence.
function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

// Fixtures written as blueprints directly (worldBlueprint), and those
// drawn under a render mask of their own rather than renderMask's.
const DIRECT = new Set(['rotated-elements', 'liquids', 'light', 'materials', 'cache-origin']);
const MASKS = { 'cache-origin': { minX: 0, maxX: 127, minZ: 0, maxZ: 127 } };
const FIXTURES = {
  'stairs-slabs'(world) {
    const items = [];
    for (const block of ['oak_stairs', 'stone_brick_stairs', 'cut_copper_stairs']) {
      for (const facing of FACINGS) for (const half of ['bottom', 'top']) {
        for (const shape of ['straight', 'inner_left', 'inner_right', 'outer_left', 'outer_right']) {
          items.push([`minecraft:${block}`, { facing, half, shape, waterlogged: 'false' }]);
        }
      }
    }
    for (const block of ['oak_slab', 'stone_slab', 'smooth_stone_slab', 'cut_sandstone_slab']) {
      for (const type of ['bottom', 'top', 'double']) items.push([`minecraft:${block}`, { type, waterlogged: 'false' }]);
    }
    grid(world, items);
    // Touching stairs and slabs, culling each other.
    for (let x = 0; x < 8; x += 1) for (let z = 0; z < 8; z += 1) {
      world.set(x, 1, 40 + z, 'minecraft:stone_brick_stairs', { facing: FACINGS[(x + z) % 4], half: x % 2 ? 'top' : 'bottom', shape: 'straight', waterlogged: 'false' });
      world.set(x, 2, 40 + z, 'minecraft:oak_slab', { type: (x + z) % 2 ? 'top' : 'bottom', waterlogged: 'false' });
    }
  },
  'rotated-elements'(world) {
    const items = [];
    for (let rotation = 0; rotation < 16; rotation += 1) {
      items.push(['minecraft:oak_sign', { rotation: String(rotation), waterlogged: 'false' }]);
      items.push(['minecraft:white_banner', { rotation: String(rotation) }]);
      items.push(['minecraft:skeleton_skull', { rotation: String(rotation), powered: 'false' }]);
      items.push(['minecraft:cherry_hanging_sign', { rotation: String(rotation), attached: 'false', waterlogged: 'false' }]);
    }
    for (const facing of FACINGS) {
      items.push(['minecraft:wall_torch', { facing }], ['minecraft:soul_wall_torch', { facing }], ['minecraft:redstone_wall_torch', { facing, lit: 'true' }]);
      items.push(['minecraft:tripwire_hook', { facing, attached: 'false', powered: 'false' }]);
      items.push(['minecraft:repeater', { facing, delay: '2', locked: 'false', powered: 'false' }]);
      items.push(['minecraft:comparator', { facing, mode: 'subtract', powered: 'true' }]);
      items.push(['minecraft:bell', { facing, attachment: 'single_wall', powered: 'false' }]);
      items.push(['minecraft:grindstone', { facing, face: 'wall' }]);
      for (const face of ['floor', 'wall', 'ceiling']) items.push(['minecraft:lever', { face, facing, powered: face === 'wall' ? 'true' : 'false' }]);
    }
    for (const facing of ['up', 'down', 'north', 'south', 'west', 'east']) {
      items.push(['minecraft:end_rod', { facing }], ['minecraft:lightning_rod', { facing, powered: 'false', waterlogged: 'false' }]);
      items.push(['minecraft:amethyst_cluster', { facing, waterlogged: 'false' }]);
    }
    items.push(['minecraft:torch'], ['minecraft:soul_torch'], ['minecraft:redstone_torch', { lit: 'true' }],
      ['minecraft:brewing_stand', { has_bottle_0: 'true', has_bottle_1: 'false', has_bottle_2: 'true' }],
      ['minecraft:short_grass'], ['minecraft:poppy'], ['minecraft:sugar_cane', { age: '0' }], ['minecraft:cobweb'],
      ['minecraft:lantern', { hanging: 'true', waterlogged: 'false' }], ['minecraft:campfire', { facing: 'east', lit: 'true', signal_fire: 'false', waterlogged: 'false' }],
      ['minecraft:iron_chain', { axis: 'x', waterlogged: 'false' }], ['minecraft:iron_chain', { axis: 'z', waterlogged: 'false' }],
      ['minecraft:hopper', { facing: 'east', enabled: 'true' }], ['minecraft:anvil', { facing: 'west' }], ['minecraft:cauldron'],
      ['minecraft:potted_fern'], ['minecraft:potted_bamboo'], ['minecraft:pointed_dripstone', { thickness: 'tip', vertical_direction: 'up', waterlogged: 'false' }],
      ['minecraft:rail', { shape: 'ascending_east', waterlogged: 'false' }], ['minecraft:powered_rail', { shape: 'north_south', powered: 'true', waterlogged: 'false' }],
      ['minecraft:daylight_detector', { inverted: 'false', power: '0' }], ['minecraft:scaffolding', { bottom: 'false', distance: '0', waterlogged: 'false' }],
      ['minecraft:big_dripleaf', { facing: 'north', tilt: 'partial', waterlogged: 'false' }], ['minecraft:sea_pickle', { pickles: '3', waterlogged: 'false' }]);
    grid(world, items);
  },
  liquids(world) {
    for (let level = 0; level < 16; level += 1) {
      world.set(level * 3, 0, 0, 'minecraft:stone');
      world.set(level * 3, 1, 0, 'minecraft:water', { level: String(level) });
      world.set(level * 3, 0, 4, 'minecraft:stone');
      world.set(level * 3, 1, 4, 'minecraft:lava', { level: String(level) });
    }
    // Pools of mixed levels, for every flow direction and corner height.
    const next = random(7);
    for (const [liquid, z0] of [['minecraft:water', 10], ['minecraft:lava', 30]]) {
      for (let x = 0; x < 14; x += 1) for (let z = 0; z < 14; z += 1) {
        world.set(x, 0, z0 + z, 'minecraft:stone');
        const roll = next();
        if (roll < 0.12) continue;
        if (roll < 0.18) { world.set(x, 1, z0 + z, 'minecraft:stone'); continue; }
        world.set(x, 1, z0 + z, liquid, { level: String(Math.floor(next() * (next() < 0.2 ? 16 : 8))) });
        if (next() < 0.15) world.set(x, 2, z0 + z, liquid, { level: String(Math.floor(next() * 16)) });
      }
    }
    // A waterfall, and water over glass and under ice.
    for (let y = 1; y < 8; y += 1) world.set(20, y, 0, 'minecraft:water', { level: String(8 + (y % 8)) });
    world.set(22, 1, 0, 'minecraft:glass');
    world.set(22, 2, 0, 'minecraft:water', { level: '0' });
    world.set(22, 3, 0, 'minecraft:ice');
  },
  waterlogged(world) {
    const items = [];
    for (const [id, states] of [
      ['minecraft:oak_stairs', { facing: 'east', half: 'bottom', shape: 'straight' }],
      ['minecraft:oak_slab', { type: 'bottom' }], ['minecraft:oak_slab', { type: 'top' }],
      ['minecraft:oak_fence', { north: 'true', south: 'false', east: 'true', west: 'false' }],
      ['minecraft:glass_pane', { north: 'true', south: 'true', east: 'false', west: 'false' }],
      ['minecraft:chest', { facing: 'south', type: 'single' }], ['minecraft:lantern', { hanging: 'false' }],
      ['minecraft:oak_trapdoor', { facing: 'north', half: 'top', open: 'false', powered: 'false' }],
      ['minecraft:oak_sign', { rotation: '4' }], ['minecraft:iron_chain', { axis: 'y' }],
      ['minecraft:conduit'], ['minecraft:sea_pickle', { pickles: '2' }], ['minecraft:tube_coral'],
    ]) items.push([id, { ...states, waterlogged: 'true' }]);
    items.push(['minecraft:seagrass'], ['minecraft:kelp', { age: '3' }], ['minecraft:kelp_plant']);
    grid(world, items);
    // In and beside open water.
    for (let x = 0; x < 10; x += 1) for (let z = 0; z < 6; z += 1) {
      world.set(x, 0, 20 + z, 'minecraft:sand');
      if (x % 3 === 0 && z % 2 === 0) world.set(x, 1, 20 + z, 'minecraft:oak_stairs', { facing: FACINGS[z % 4], half: 'bottom', shape: 'straight', waterlogged: 'true' });
      else world.set(x, 1, 20 + z, 'minecraft:water', { level: String((x * 3 + z) % 8) });
      world.set(x, 2, 20 + z, x % 2 ? 'minecraft:seagrass' : 'minecraft:water', x % 2 ? {} : { level: '0' });
    }
    world.set(14, 0, 20, 'minecraft:soul_sand');
    for (let y = 1; y < 5; y += 1) world.set(14, y, 20, 'minecraft:bubble_column', { drag: 'false' });
    world.set(14, 1, 22, 'minecraft:tall_seagrass', { half: 'lower' });
    world.set(14, 2, 22, 'minecraft:tall_seagrass', { half: 'upper' });
  },
  tints(world) {
    const items = [['minecraft:grass_block', { snowy: 'false' }], ['minecraft:short_grass'], ['minecraft:fern'], ['minecraft:vine', { up: 'true', north: 'true', south: 'false', east: 'true', west: 'false' }],
      ['minecraft:lily_pad'], ['minecraft:sugar_cane', { age: '3' }], ['minecraft:potted_fern'], ['minecraft:water_cauldron', { level: '2' }],
      ['minecraft:mangrove_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' }], ['minecraft:spruce_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' }],
      ['minecraft:birch_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' }], ['minecraft:cherry_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' }],
      ['minecraft:azalea_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' }], ['minecraft:leaf_litter', { facing: 'north', segment_amount: '3' }]];
    for (const wood of ['oak', 'jungle', 'acacia', 'dark_oak', 'pale_oak']) items.push([`minecraft:${wood}_leaves`, { distance: '1', persistent: 'true', waterlogged: 'false' }]);
    for (let age = 0; age < 8; age += 1) items.push(['minecraft:melon_stem', { age: String(age) }], ['minecraft:pumpkin_stem', { age: String(age) }]);
    for (let power = 0; power < 16; power += 1) items.push(['minecraft:redstone_wire', { power: String(power), north: 'side', south: 'side', east: 'none', west: 'none' }]);
    grid(world, items, { floor: 'minecraft:grass_block' });
    world.set(0, 0, 40, 'minecraft:grass_block', { snowy: 'false' });
    world.set(0, 1, 40, 'minecraft:tall_grass', { half: 'lower' });
    world.set(0, 2, 40, 'minecraft:tall_grass', { half: 'upper' });
    world.set(2, 0, 40, 'minecraft:melon');
    world.set(2, 0, 41, 'minecraft:attached_melon_stem', { facing: 'north' });
  },
  'random-offset'(world) {
    const plants = ['short_grass', 'fern', 'dandelion', 'poppy', 'blue_orchid', 'allium', 'azure_bluet', 'red_tulip', 'oxeye_daisy', 'cornflower', 'lily_of_the_valley', 'wither_rose', 'crimson_roots', 'warped_roots', 'nether_sprouts', 'small_dripleaf'];
    for (let x = -40; x < 40; x += 3) for (let z = -40; z < 40; z += 5) {
      const plant = plants[((x * 7 + z * 13) % plants.length + plants.length) % plants.length];
      world.set(x, 0, z, plant === 'crimson_roots' || plant === 'warped_roots' || plant === 'nether_sprouts' ? 'minecraft:crimson_nylium' : 'minecraft:grass_block');
      world.set(x, 1, z, `minecraft:${plant}`, plant === 'small_dripleaf' ? { facing: 'east', half: 'lower', waterlogged: 'false' } : {});
    }
    for (const [i, flower] of ['rose_bush', 'peony', 'lilac', 'sunflower'].entries()) {
      world.set(-50 + i * 2, 0, -50, 'minecraft:grass_block');
      world.set(-50 + i * 2, 1, -50, `minecraft:${flower}`, { half: 'lower' });
      world.set(-50 + i * 2, 2, -50, `minecraft:${flower}`, { half: 'upper' });
    }
  },
  multipart(world) {
    const bool = (n, bit) => (n >> bit & 1 ? 'true' : 'false');
    const items = [];
    for (let n = 0; n < 16; n += 1) {
      const sides = { north: bool(n, 0), south: bool(n, 1), east: bool(n, 2), west: bool(n, 3) };
      items.push(['minecraft:oak_fence', { ...sides, waterlogged: 'false' }], ['minecraft:glass_pane', { ...sides, waterlogged: 'false' }], ['minecraft:iron_bars', { ...sides, waterlogged: 'false' }]);
      items.push(['minecraft:cobblestone_wall', { north: n & 1 ? 'low' : 'none', south: n & 2 ? 'tall' : 'none', east: n & 4 ? 'low' : 'none', west: n & 8 ? 'tall' : 'none', up: bool(n, 1), waterlogged: 'false' }]);
      items.push(['minecraft:redstone_wire', { north: n & 1 ? 'up' : 'side', south: n & 2 ? 'side' : 'none', east: n & 4 ? 'side' : 'none', west: n & 8 ? 'up' : 'none', power: String(n) }]);
      items.push(['minecraft:brown_mushroom_block', { north: bool(n, 0), south: bool(n, 1), east: bool(n, 2), west: bool(n, 3), up: bool(n, 0), down: bool(n, 2) }]);
      items.push(['minecraft:glow_lichen', { north: bool(n, 0), south: bool(n, 1), east: bool(n, 2), west: bool(n, 3), up: 'false', down: bool(n, 1), waterlogged: 'false' }]);
      items.push(['minecraft:chiseled_bookshelf', { facing: FACINGS[n % 4], slot_0_occupied: bool(n, 0), slot_1_occupied: bool(n, 1), slot_2_occupied: bool(n, 2), slot_3_occupied: bool(n, 3), slot_4_occupied: 'true', slot_5_occupied: 'false' }]);
    }
    grid(world, items, { width: 12 });
    // A fence and wall run, connected by the engine.
    for (let x = 0; x < 10; x += 1) {
      world.set(x, 1, 60, 'minecraft:spruce_fence');
      world.set(x, 1, 62, 'minecraft:andesite_wall');
      world.set(x, 1, 64, 'minecraft:red_stained_glass_pane');
      world.set(x, 0, 60, 'minecraft:stone'); world.set(x, 0, 62, 'minecraft:stone'); world.set(x, 0, 64, 'minecraft:stone');
    }
  },
  'glass-culling'(world) {
    for (let x = 0; x < 6; x += 1) for (let y = 0; y < 6; y += 1) for (let z = 0; z < 6; z += 1) world.set(x, y, z, 'minecraft:glass');
    for (let i = 0; i < 16; i += 1) for (let y = 0; y < 3; y += 1) world.set(10 + i, y, 0, `minecraft:${COLORS[i]}_stained_glass`);
    for (let x = 0; x < 4; x += 1) for (let y = 0; y < 4; y += 1) {
      world.set(x, y, 10, 'minecraft:ice'); world.set(x, y, 11, 'minecraft:ice');
      world.set(x, y, 13, 'minecraft:tinted_glass'); world.set(x, y, 14, 'minecraft:glass');
      world.set(x, y, 16, 'minecraft:stone'); world.set(x, y, 17, 'minecraft:glass');
      world.set(x, y, 19, 'minecraft:oak_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' });
      world.set(x, y, 20, 'minecraft:slime_block'); world.set(x, y, 21, 'minecraft:honey_block');
    }
  },
  'block-entities'(world) {
    const items = [];
    for (const facing of FACINGS) {
      items.push(['minecraft:chest', { facing, type: 'single', waterlogged: 'false' }], ['minecraft:trapped_chest', { facing, type: 'single', waterlogged: 'false' }],
        ['minecraft:ender_chest', { facing, waterlogged: 'false' }], ['minecraft:copper_chest', { facing, type: 'single', waterlogged: 'false' }]);
      items.push(['minecraft:decorated_pot', { facing, cracked: 'false', waterlogged: 'false' }], ['minecraft:lectern', { facing, has_book: 'true', powered: 'false' }]);
      for (const wood of WOODS.slice(0, 4)) items.push([`minecraft:${wood}_wall_sign`, { facing, waterlogged: 'false' }], [`minecraft:${wood}_wall_hanging_sign`, { facing, waterlogged: 'false' }]);
      items.push(['minecraft:red_wall_banner', { facing }], ['minecraft:creeper_wall_head', { facing, powered: 'false' }], ['minecraft:player_wall_head', { facing, powered: 'false' }]);
    }
    for (const wood of WOODS) items.push([`minecraft:${wood}_sign`, { rotation: '0', waterlogged: 'false' }], [`minecraft:${wood}_hanging_sign`, { rotation: '8', attached: 'true', waterlogged: 'false' }]);
    for (const color of COLORS) items.push([`minecraft:${color}_shulker_box`, { facing: 'up' }], [`minecraft:${color}_banner`, { rotation: '3' }]);
    for (const head of ['zombie_head', 'creeper_head', 'dragon_head', 'piglin_head', 'wither_skeleton_skull', 'player_head']) items.push([`minecraft:${head}`, { rotation: '5', powered: 'false' }]);
    items.push(['minecraft:shulker_box', { facing: 'north' }], ['minecraft:conduit', { waterlogged: 'false' }], ['minecraft:enchanting_table'], ['minecraft:spawner'],
      ['minecraft:end_portal_frame', { eye: 'true', facing: 'south' }], ['minecraft:bell', { facing: 'north', attachment: 'floor', powered: 'false' }],
      ['minecraft:cake', { bites: '2' }], ['minecraft:candle_cake', { lit: 'true' }]);
    grid(world, items);
    // Double chests and beds, each half beside its other half.
    COLORS.forEach((color, i) => {
      const facing = FACINGS[i % 4];
      const [dx, dz] = { north: [0, -1], south: [0, 1], west: [-1, 0], east: [1, 0] }[facing];
      const x = i * 4; const z = 60;
      world.set(x, 0, z, 'minecraft:stone'); world.set(x + dx, 0, z + dz, 'minecraft:stone');
      world.set(x, 1, z, `minecraft:${color}_bed`, { facing, part: 'foot', occupied: 'false' });
      world.set(x + dx, 1, z + dz, `minecraft:${color}_bed`, { facing, part: 'head', occupied: 'false' });
    });
    FACINGS.forEach((facing, i) => {
      const [dx, dz] = { north: [1, 0], south: [-1, 0], west: [0, -1], east: [0, 1] }[facing];
      const x = i * 4; const z = 70;
      world.set(x, 1, z, 'minecraft:chest', { facing, type: 'left', waterlogged: 'false' });
      world.set(x - dx, 1, z - dz, 'minecraft:chest', { facing, type: 'right', waterlogged: 'false' });
    });
  },
  light(world) {
    // A closed room lit by a torch, a room under a slab roof, an overhang.
    for (let x = 0; x < 9; x += 1) for (let y = 0; y < 6; y += 1) for (let z = 0; z < 9; z += 1) {
      const edge = x === 0 || x === 8 || y === 0 || y === 5 || z === 0 || z === 8;
      if (edge) world.set(x, y, z, 'minecraft:stone_bricks');
    }
    world.set(4, 1, 4, 'minecraft:torch');
    world.set(2, 1, 2, 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'false' });
    world.set(6, 2, 6, 'minecraft:redstone_lamp', { lit: 'true' });
    for (let x = 12; x < 22; x += 1) for (let z = 0; z < 10; z += 1) {
      world.set(x, 0, z, 'minecraft:stone');
      world.set(x, 4, z, (x + z) % 3 ? 'minecraft:smooth_stone_slab' : 'minecraft:glass', (x + z) % 3 ? { type: 'top', waterlogged: 'false' } : {});
    }
    world.set(15, 1, 3, 'minecraft:glowstone'); world.set(18, 1, 6, 'minecraft:sea_lantern'); world.set(13, 1, 8, 'minecraft:magma_block');
    world.set(20, 1, 2, 'minecraft:lava', { level: '0' }); world.set(16, 1, 7, 'minecraft:end_rod', { facing: 'up' });
    world.set(19, 3, 8, 'minecraft:light', { level: '11', waterlogged: 'false' });
    for (let x = 25; x < 33; x += 1) for (let z = 0; z < 8; z += 1) {
      world.set(x, 0, z, 'minecraft:grass_block', { snowy: 'false' });
      if (x > 27) world.set(x, 6, z, z % 2 ? 'minecraft:oak_leaves' : 'minecraft:oak_trapdoor', z % 2 ? { distance: '1', persistent: 'true', waterlogged: 'false' } : { facing: 'north', half: 'top', open: String(x % 2 === 0), powered: 'false', waterlogged: 'false' });
    }
    for (let y = 0; y < 40; y += 1) world.set(36, y, 4, y % 5 ? 'minecraft:stone' : 'minecraft:glass');
    world.set(38, 0, 4, 'minecraft:water', { level: '0' }); world.set(38, 1, 4, 'minecraft:ice');
  },
  'negative-coordinates'(world) {
    const blocks = ['minecraft:stone', 'minecraft:oak_planks', 'minecraft:glass', 'minecraft:grass_block', 'minecraft:oak_leaves'];
    for (const [ox, oz] of [[-200, -180], [-33, -31], [-2, 30], [29, -3]]) {
      for (let x = 0; x < 7; x += 1) for (let z = 0; z < 7; z += 1) for (let y = 0; y < 3; y += 1) {
        if ((x + y + z) % 4 === 0) continue;
        const id = blocks[(x * 3 + z + y) % blocks.length];
        world.set(ox + x, y - 20, oz + z, id, id === 'minecraft:oak_leaves' ? { distance: '1', persistent: 'true', waterlogged: 'false' } : {});
      }
      world.set(ox + 3, 1 - 20 + 3, oz + 3, 'minecraft:poppy');
      world.set(ox + 3, -20 + 3, oz + 3, 'minecraft:grass_block');
    }
  },
  // Every state of each block with at most 16 of them and every other
  // block's default state, apart, in one tile: over a thousand material
  // groups.
  materials(world, table) {
    const states = [];
    for (const [id, list] of table) {
      if (/^minecraft:(air|cave_air|void_air)$/.test(id)) continue;
      states.push(...(list.length <= 16 ? list : [{}]).map((properties) => [id, properties]));
    }
    let i = 0;
    for (let y = 0; i < states.length; y += 2) {
      for (let x = 2; x < 33 && i < states.length; x += 2) for (let z = 2; z < 33 && i < states.length; z += 2) {
        world.set(x + (y / 2 % 2), y, z, states[i][0], states[i][1]);
        i += 1;
      }
    }
  },
  // BlockNeighborhood's quirk at 0, 0, 0 (see renderer/src/mesh.rs): with
  // the mask starting at x = z = 0, the tile over the origin first meets
  // its cache entry 0 at Minecraft 0, 0, 0, which then reads the block above.
  'cache-origin'(world) {
    const y = -Y_OFFSET;
    world.set(0, y, 0, 'minecraft:glass');
    world.set(0, y + 1, 0, 'minecraft:stone');
    world.set(0, y + 2, 0, 'minecraft:grass_block', { snowy: 'false' });
    world.set(0, y + 3, 0, 'minecraft:short_grass');
    world.set(1, y, 0, 'minecraft:water', { level: '0' });
    world.set(1, y + 1, 0, 'minecraft:oak_stairs', { facing: 'west', half: 'bottom', shape: 'straight', waterlogged: 'true' });
    world.set(0, y, 1, 'minecraft:oak_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' });
    world.set(1, y, 1, 'minecraft:glowstone');
    world.set(2, y, 2, 'minecraft:grass_block', { snowy: 'false' });
    world.set(2, y + 1, 2, 'minecraft:fern');
  },
  // A tile past BlueMap's million-face limit: solid leaves, every face drawn.
  'face-limit'(world) {
    for (let y = 0; y < 90; y += 1) for (let x = 2; x <= 33; x += 1) for (let z = 2; z <= 33; z += 1) {
      world.set(x, y, z, 'minecraft:oak_leaves', { distance: '1', persistent: 'true', waterlogged: 'false' });
    }
  },
};

function compare(expected, actual, blueprint) {
  const keys = [...new Set([...expected.keys(), ...actual.keys()])].sort();
  let same = 0;
  const differences = [];
  for (const key of keys) {
    const java = expected.get(key); const wasm = actual.get(key);
    if (java && wasm && java.equals(wasm)) { same += 1; continue; }
    if (!java || !wasm) differences.push(`    ${key}: only ${java ? 'BlueMap' : 'wasm'} drew it`);
    else if (differences.length < 3) differences.push(describeDifference(key, java, wasm, blueprint));
    else differences.push(`    ${key}: differs`);
  }
  return { same, total: keys.length, differences };
}

async function main() {
  const limit = Number(option('limit', Infinity));
  const match = option('match', '');
  build();
  execFileSync('cargo', ['build', '--release'], { cwd: RENDERER, stdio: 'inherit' });
  const blockTable = await minecraftLive.blockTable();
  const bluemap = await bake(blockTable);
  const mesher = await loadMesher(bluemap.pack);
  // Each block's states in the block table, as property objects.
  const table = new Map();
  for (const line of fs.readFileSync(blockTable, 'utf8').split('\n').filter(Boolean)) {
    const [name] = line.split('\t');
    if (name === 'default') continue;
    const [id, properties = ''] = name.replace(/]$/, '').split('[');
    if (!table.has(id)) table.set(id, []);
    table.get(id).push(Object.fromEntries(properties ? properties.split(',').map((pair) => pair.split('=')) : []));
  }

  const cases = [];
  if (!flag('no-fixtures')) {
    for (const [name, fill] of Object.entries(FIXTURES)) {
      cases.push([`fixture:${name}`, () => { const world = new World(); fill(world, table); return world; }, DIRECT.has(name)]);
    }
  }
  if (!flag('no-builds')) {
    const builds = new Map(corpusArtifacts());
    for (const [name, file] of [...builds].sort(([a], [b]) => a.localeCompare(b))) {
      cases.push([name, () => {
        const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
        const world = new World();
        for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
        return world;
      }]);
    }
  }

  const keep = option('keep', null);
  const save = (name, label, sets) => {
    for (const [side, tiles] of sets) {
      for (const [key, bytes] of tiles) {
        const file = path.join(keep, name.replace(/[:/]/g, '-'), label, side, key);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, bytes);
      }
    }
  };
  const results = [];
  for (const [name, make, direct] of cases.filter(([name]) => name.includes(match)).slice(0, limit)) {
    const result = { name };
    try {
      const world = make();
      world.finalize();
      result.blocks = world.size();
      const mask = MASKS[name.replace(/^fixture:/, '')] || renderMask(world.bounds());
      // Fixtures whose exact states placing would change are written as
      // blueprints directly.
      const blueprint = direct ? worldBlueprint(world, Y_OFFSET) : await minecraftLive.placeBlueprint(null, world, null);
      let started = Date.now();
      const expected = await bluemap.render(blueprint, mask);
      result.javaMs = Date.now() - started;
      started = process.hrtime.bigint();
      const actual = mesher.mesh(blueprint, mask);
      result.wasmMs = Number(process.hrtime.bigint() - started) / 1e6;
      const states = readBlueprint(blueprint);
      const { same, total, differences } = compare(expected, actual, states);
      Object.assign(result, {
        same,
        total,
        faces: [...expected.values()].reduce((sum, tile) => sum + (tile[2] | tile[3] << 8 | tile[4] << 16) / 3, 0),
        groups: Math.max(0, ...[...expected.values()].map((tile) => decodePrbm(tile).groups.length)),
      });
      if (differences.length) result.differences = differences;
      if (keep && same !== total) save(name, 'full', [['bluemap', expected], ['wasm', actual]]);

      // The edit: BlueMap draws it afresh; the module redraws what it reports.
      const after = edited(states);
      const expectedEdit = await bluemap.render(after, mask);
      const edit = mesher.edit(after, mask, actual);
      const comparedEdit = compare(expectedEdit, edit.tiles, readBlueprint(after));
      result.edit = { same: comparedEdit.same, total: comparedEdit.total, redrawn: edit.changed };
      if (comparedEdit.differences.length) result.edit.differences = comparedEdit.differences;
      if (keep && comparedEdit.same !== comparedEdit.total) save(name, 'edit', [['bluemap', expectedEdit], ['wasm', edit.tiles]]);
    } catch (error) {
      result.error = String(error.stack || error).slice(0, 2000);
    }
    results.push(result);
    console.log(`${name}: ${result.blocks ?? '?'} blocks, ${result.error ? `error: ${result.error}`
      : `tiles identical ${result.same}/${result.total}, ${result.faces} faces (at most ${result.groups} groups a tile), BlueMap ${result.javaMs} ms, wasm ${result.wasmMs.toFixed(1)} ms; `
        + `edit identical ${result.edit.same}/${result.edit.total} with ${result.edit.redrawn} redrawn`}`);
    for (const line of [...result.differences || [], ...result.edit?.differences || []]) console.log(line);
  }
  await minecraftLive.engine.stop();
  bluemap.stop();
  const exact = results.filter((result) => !result.error && result.same === result.total && result.edit.same === result.edit.total);
  const fixtures = results.filter((result) => result.name.startsWith('fixture:'));
  const javaMs = results.reduce((sum, result) => sum + (result.javaMs || 0), 0);
  const wasmMs = results.reduce((sum, result) => sum + (result.wasmMs || 0), 0);
  const redrawn = results.reduce((sum, result) => sum + (result.edit?.redrawn || 0), 0);
  const editTiles = results.reduce((sum, result) => sum + (result.edit?.total || 0), 0);
  console.log(`\n${exact.length}/${results.length} byte-identical, full and edited (fixtures ${fixtures.filter((r) => exact.includes(r)).length}/${fixtures.length}, builds ${exact.length - fixtures.filter((r) => exact.includes(r)).length}/${results.length - fixtures.length}); BlueMap ${javaMs} ms, wasm ${wasmMs.toFixed(0)} ms in all; edits redrew ${redrawn} of ${editTiles} tiles`);
  console.log(`wasm ${fs.statSync(MODULE).size} bytes, pack ${bluemap.pack.length} bytes`);
  const report = option('report', null);
  if (report) fs.writeFileSync(report, JSON.stringify(results, null, 2));
  // A build the engine refuses to place ("unported: ...") reaches neither
  // mesher: counted apart, as scripts/core-parity.js counts refusals.
  const refused = results.filter((result) => /The engine failed: unported:/.test(result.error || '')).length;
  const errored = results.filter((result) => result.error).length - refused;
  const differed = results.length - exact.length - errored - refused;
  console.log(`${exact.length} matched, ${differed} differed, 0 refused by both, ${refused} refused by the engine's placement, ${errored} errored`);
  // A run that compared nothing proves nothing.
  return !differed && !errored && exact.length + differed > 0;
}

main().then((passed) => process.exit(passed ? 0 : 1), (error) => { console.error(error); process.exit(1); });
