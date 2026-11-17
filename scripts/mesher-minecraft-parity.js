'use strict';

// The mesher (renderer/) against Minecraft's own client renderer. For each
// world, the native mesher (renderer's `mcbench-mesh minecraft`) writes the
// world as it reads it (each cell's state and light) and every quad of every
// tile it draws, with the block each quad belongs to (tile::Source); the
// client's section renderer, run in the bake's JVM (server/McbenchTerrain.java:
// ModelBlockRenderer and LiquidBlockRenderer, as SectionCompiler runs them,
// smooth lighting on), draws the same world. The two sets of quads are
// compared as multisets, block by block, and every difference is counted by
// class and by block.
//
//   MCBENCH_DATA_DIR=<scratch copy> [MCBENCH_CORPUS=<corpus>] node scripts/mesher-minecraft-parity.js
//     [--sets states,faces,arrangements,liquids,fixtures,builds,worlds] [--match text] [--limit n]
//     [--worlds <copy of mcbench.sqlite3>] [--max-blocks n] [--report file.json] [--examples n]
//
// The sets: `states`, every block state of the block table alone in air in
// open sky, at three places (random model variants and offsets come from the
// position); `faces`, every state with a stone block against one of its
// faces, a world for each face; `arrangements`, the blocks that connect or
// attach placed by the engine in the arrangements Minecraft's placement makes
// of them (fences, walls, panes and bars in runs and corners, fence gates
// open and closed in fences and walls, buttons and levers on every face of a
// block, scaffolding stacked and overhanging, hoppers facing every way and
// waterlogged), the blocks chosen by their classes in Minecraft's own data
// (server/minecraft-facts.js); `liquids`, water and lava at every level
// beside, above and below every level of the same fluid and of the other,
// and every waterlogged state in water; `fixtures`, render-parity's fixtures;
// `builds`, the community builds of MCBENCH_CORPUS as the engine places them;
// `worlds`, every chat's saved world in the data folder whose database
// --worlds names (a copy of mcbench.sqlite3, its worlds/ folder beside it),
// as the engine places it.
//
// Matching. Quads are matched within the block that draws them (its
// position, and whether model or fluid): a mesher quad and a Minecraft quad
// match when their four corners are the same points (each within POSITION
// of one of the other's), one facing the same way first; one wound the other
// way is a difference of its own. A matched pair is then
// compared on its texture, each corner's texture coordinates (within UV of
// the texture's width and height), the tint, each corner's ambient occlusion
// (the mesher's, against Minecraft's brightness divided by the face's shade,
// ClientLevel.getShade, which the viewers apply by the face's direction
// instead; within AO) and each corner's sky and block light (the mesher
// lights a face by one level of each, Minecraft each corner by smooth
// lighting's blend of four cells, sixteenths of a level; within LIGHT). The
// tolerances: both renderers compute positions and coordinates in floats by
// different steps (BlueMap's element matrices, Minecraft's FaceBakery), so
// equal values differ in their last bits, far below a thousandth of a block
// or a five-hundredth of a texture; the capture's own atlas shrinks texture
// coordinates toward a face's middle by 4/65,536 of the texture (see
// McbenchTerrain.java).
//
// Quads left over are grouped again by block, facing and texture to tell
// apart quads placed differently (`position`) from quads only one side draws.
// Block entity renderers' quads (tile::Source ENTITY) are not drawn by the
// section renderer; they are counted apart (McbenchBlockEntities captures
// them from Minecraft's own renderers).

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
const { blocksBlueprint, worldBlueprint, readBlueprint } = require('../server/blueprint');
const { MC_VERSION } = require('../server/assets');
const { JAVA, JAVAC } = require('../server/tools');
const { World } = require('../server/world');
const { Y_OFFSET, BIOME } = require('../server/minecraft-dimension');
const { minecraftFacts } = require('../server/minecraft-facts');
const { BlockStates } = require('../server/block-states');
const { TARGET } = require('./build-renderer');
const { corpusArtifacts } = require('./corpus');
const { FIXTURES, DIRECT, MASKS } = require('./render-parity');

const POSITION = 1e-3;
const UV = 2e-3;
const AO = 1e-3;
const LIGHT = 1e-6;
const TINT = 1e-4;

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};
const RENDERER = path.join(ROOT, 'renderer');
const FOLDER = path.join(path.dirname(blueMap.root), 'mesher-minecraft');
// The native mesher, copied into the run's folder once built, so a build
// made while the run goes on does not change what it runs.
const MESH = path.join(FOLDER, 'mcbench-mesh');

function writeFiles(folder, files) {
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(folder, name)), { recursive: true });
    fs.writeFileSync(path.join(folder, name), text);
  }
}

// The pack baked as the app bakes it (McbenchBake, in a folder of its own),
// and Minecraft's renderer started once in a JVM of the client's classes.
async function start(blockTable) {
  await blueMap.ensureDownloads();
  const libraries = (await minecraftLive.ensurePaper()).filter((entry) => entry.startsWith('libraries/')).map((entry) => path.join(minecraftLive.paperDir, entry));
  fs.rmSync(FOLDER, { recursive: true, force: true });
  writeFiles(FOLDER, configFiles(path.join(blueMap.root, 'data')));
  fs.copyFileSync(path.join(TARGET, 'release', 'mcbench-mesh'), MESH);
  const classes = path.join(FOLDER, 'classes');
  execFileSync(JAVAC, ['-proc:none', '-nowarn', '-classpath', blueMap.jar, '-d', classes,
    ...['McbenchBake.java', 'McbenchEntityAssets.java', 'McbenchClient.java', 'McbenchBlockEntities.java', 'McbenchEntities.java', 'McbenchRenderStates.java', 'McbenchTerrain.java'].map((name) => path.join(ROOT, 'server', name))], { stdio: 'inherit' });
  const classpath = [blueMap.jar, classes].join(path.delimiter);
  execFileSync(JAVA, ['-Xmx2G', '-cp', classpath, 'McbenchBake', MC_VERSION, blockTable, libraries.join(path.delimiter), BIOME, blueMap.mappings],
    { cwd: FOLDER, stdio: ['ignore', 'inherit', 'inherit'] });
  const clientJar = path.join(blueMap.root, 'data', `minecraft-client-${MC_VERSION}.jar`);
  const java = spawn(JAVA, ['-Xmx6G', '-cp', classpath, 'McbenchTerrain', clientJar, blueMap.mappings, blockTable, libraries.join(path.delimiter), BIOME],
    { cwd: FOLDER, stdio: ['pipe', 'pipe', 'inherit'] });
  const replies = readline.createInterface({ input: java.stdout })[Symbol.asyncIterator]();
  const reply = async (prefix) => {
    for (;;) {
      const { value, done } = await replies.next();
      if (done) throw new Error('Minecraft\'s renderer stopped');
      // Minecraft's bootstrap sends System.out through its logger, which
      // prefixes each line.
      const at = value.indexOf(prefix);
      if (at >= 0) return value.slice(at);
    }
  };
  await reply('MCBENCH_READY');
  const textures = JSON.parse(fs.readFileSync(path.join(FOLDER, 'web', 'maps', 'live', 'textures.json'), 'utf8')).map((texture) => texture.resourcePath);
  const pack = path.join(FOLDER, 'web', 'mcbench-pack.bin');
  let drawn = 0;
  // Both sides' quads of a blueprint (bytes) under a render mask.
  const draw = async (blueprint, mask) => {
    const name = path.join(FOLDER, `case-${drawn += 1}`);
    fs.writeFileSync(`${name}.blueprint`, blueprint);
    let started = process.hrtime.bigint();
    execFileSync(MESH, ['minecraft', pack, `${name}.blueprint`, mask.minX, mask.maxX, mask.minZ, mask.maxZ, `${name}.scene`, `${name}.quads`].map(String), { stdio: 'inherit' });
    const meshMs = Number(process.hrtime.bigint() - started) / 1e6;
    started = process.hrtime.bigint();
    java.stdin.write(`${name}.scene ${name}.capture\n`);
    await reply('MCBENCH_CAPTURED ');
    const captureMs = Number(process.hrtime.bigint() - started) / 1e6;
    const mesher = readQuads(fs.readFileSync(`${name}.quads`), textures);
    const minecraft = readCapture(fs.readFileSync(`${name}.capture`));
    for (const suffix of ['.blueprint', '.scene', '.quads', '.capture']) fs.rmSync(`${name}${suffix}`, { force: true });
    return { mesher, minecraft, meshMs, captureMs };
  };
  return { draw, stop: () => java.stdin.end() };
}

// mcbench-mesh's quads (its file documents the format).
function readQuads(bytes, textures) {
  if (bytes.toString('ascii', 0, 4) !== 'MCBQ' || bytes.readUInt32LE(4) !== 1) throw new Error('Not a version 1 quads file');
  const quads = [];
  for (let at = 8; at < bytes.length;) {
    const quad = { kind: bytes[at], x: bytes.readInt32LE(at + 1), y: bytes.readInt32LE(at + 5), z: bytes.readInt32LE(at + 9), state: bytes.readUInt32LE(at + 13), full: bytes[at + 17] === 1, corners: [] };
    at += 18;
    for (let v = 0; v < 4; v += 1) {
      const f = (k) => bytes.readFloatLE(at + k * 4);
      quad.corners.push({ p: [f(0), f(1), f(2)], uv: [f(3), f(4)], ao: f(5) });
      at += 24;
    }
    quad.tint = [bytes.readFloatLE(at), bytes.readFloatLE(at + 4), bytes.readFloatLE(at + 8)];
    quad.sky = bytes[at + 12];
    quad.block = bytes[at + 13];
    quad.texture = textures[bytes.readInt32LE(at + 14)];
    at += 18;
    quad.direction = facing(quad.corners);
    quads.push(quad);
  }
  return quads;
}

// McbenchTerrain's capture (its file documents the format).
function readCapture(bytes) {
  if (bytes.toString('ascii', 0, 4) !== 'MCBT' || bytes.readUInt32LE(4) !== 1) throw new Error('Not a version 1 capture');
  const shades = Array.from({ length: 12 }, (_, i) => bytes.readFloatLE(8 + i * 4));
  let at = 56;
  const names = [];
  for (let n = bytes.readUInt32LE(at), i = (at += 4, 0); i < n; i += 1) {
    const length = bytes.readUInt16LE(at);
    names.push(bytes.toString('utf8', at + 2, at + 2 + length));
    at += 2 + length;
  }
  const quads = [];
  while (at < bytes.length) {
    const quad = {
      kind: bytes[at], x: bytes.readInt32LE(at + 1), y: bytes.readInt32LE(at + 5), z: bytes.readInt32LE(at + 9), state: bytes.readUInt32LE(at + 13),
      direction: bytes[at + 17], cullface: bytes[at + 18], part: bytes[at + 19], flags: bytes[at + 20], tintIndex: bytes.readInt8(at + 21), emission: bytes[at + 22],
      tint: [bytes.readFloatLE(at + 23), bytes.readFloatLE(at + 27), bytes.readFloatLE(at + 31)], texture: names[bytes.readUInt16LE(at + 35)], corners: [],
    };
    at += 37;
    const shaded = quad.flags & 1;
    const shade = quad.kind === 1 ? null : shades[(shaded ? 0 : 6) + quad.direction];
    for (let v = 0; v < 4; v += 1) {
      const f = (k) => bytes.readFloatLE(at + k * 4);
      const light = bytes.readInt32LE(at + 24);
      const brightness = f(5);
      quad.corners.push({ p: [f(0), f(1), f(2)], uv: [f(3), f(4)], brightness, ao: quad.kind === 1 ? 1 : brightness / shade, sky: (light >>> 16 & 0xffff) / 16, block: (light & 0xffff) / 16, colour: bytes.readUInt32LE(at + 28) });
      at += 32;
    }
    quads.push(quad);
  }
  return { quads, shades };
}

// A quad's normal as wound (its first three corners), and a dot product.
function normal(corners) {
  const [a, b, c] = corners.map((corner) => corner.p);
  const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  return [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
}
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

// The facing of a quad's corners as wound (McbenchTerrain.facing).
function facing(corners) {
  const [a, b, c] = corners.map((corner) => corner.p);
  const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
  const [ax, ay, az] = n.map(Math.abs);
  if (ay >= ax && ay >= az) return n[1] < 0 ? 0 : 1;
  if (az >= ax) return n[2] < 0 ? 2 : 3;
  return n[0] < 0 ? 4 : 5;
}

const near = (a, b, tolerance) => Math.abs(a - b) <= tolerance;
const samePoint = (a, b) => near(a[0], b[0], POSITION) && near(a[1], b[1], POSITION) && near(a[2], b[2], POSITION);

// For each of `a`'s corners, the index of `b`'s at the same point (a
// bijection), or null.
function cornerMap(a, b) {
  const map = [];
  const used = new Set();
  for (const corner of a.corners) {
    const at = b.corners.findIndex((other, i) => !used.has(i) && samePoint(corner.p, other.p));
    if (at < 0) return null;
    used.add(at);
    map.push(at);
  }
  return map;
}

const BLOCK_ENTITY = 2;
const DIRECTIONS = ['down', 'up', 'north', 'south', 'west', 'east'];

// The differences between one world's two sets of quads, added to `tally`:
// by class, a count and the count by block.
function compareQuads(mesher, minecraft, names, tally, examples) {
  const count = (kind, state, example) => {
    const block = names[state].split('[')[0];
    const entry = tally[kind] ||= { count: 0, blocks: {}, examples: [] };
    entry.count += 1;
    entry.blocks[block] = (entry.blocks[block] || 0) + 1;
    if (example && entry.examples.length < examples) entry.examples.push(example());
  };
  const groups = new Map();
  const group = (quad) => {
    const key = `${quad.kind === 1 ? 1 : 0} ${quad.x} ${quad.y} ${quad.z}`;
    let entry = groups.get(key);
    if (!entry) groups.set(key, entry = { mesher: [], minecraft: [] });
    return entry;
  };
  let entities = 0;
  for (const quad of mesher) {
    if (quad.kind === BLOCK_ENTITY) { entities += 1; continue; }
    group(quad).mesher.push(quad);
  }
  for (const quad of minecraft.quads) group(quad).minecraft.push(quad);
  let matched = 0;
  const describe = (quad) => `${names[quad.state]} at ${quad.x},${quad.y},${quad.z} ${DIRECTIONS[quad.direction]} ${quad.texture} ${quad.corners.map((c) => `(${c.p.map((v) => v.toFixed(4)).join(',')} uv ${c.uv.map((v) => v.toFixed(4)).join(',')}${c.brightness === undefined ? ` ao ${c.ao.toFixed(3)} light ${quad.sky}/${quad.block}` : ` ao ${c.ao.toFixed(3)} light ${c.sky}/${c.block}`})`).join(' ')}`;
  for (const { mesher: ours, minecraft: theirs } of groups.values()) {
    const left = [];
    const unmatched = new Set(theirs.keys());
    for (const quad of ours) {
      let found = -1;
      let map = null;
      // A quad facing the same way first: a plane drawn both ways has two
      // quads of the same corners.
      for (const sameFacing of [true, false]) {
        for (const i of unmatched) {
          if (sameFacing !== (theirs[i].direction === quad.direction)) continue;
          map = cornerMap(quad, theirs[i]);
          if (map) { found = i; break; }
        }
        if (found >= 0) break;
      }
      if (found < 0) { left.push(quad); continue; }
      unmatched.delete(found);
      matched += 1;
      const other = theirs[found];
      const pair = () => `mesher ${describe(quad)}\n        minecraft ${describe(other)}`;
      if (dot(normal(quad.corners), normal(other.corners)) < 0) count('wound the other way (the same corners, facing back)', quad.state, pair);
      if (quad.texture !== other.texture) count('texture', quad.state, pair);
      else if (quad.corners.some((corner, v) => !near(corner.uv[0], other.corners[map[v]].uv[0], UV) || !near(corner.uv[1], other.corners[map[v]].uv[1], UV))) count('uv', quad.state, pair);
      if (quad.tint.some((value, k) => !near(value, other.tint[k], TINT))) count('tint', quad.state, pair);
      if (quad.kind !== 1 && !(other.flags & 1)) count('unshaded face (shade: false)', quad.state, pair);
      if (quad.corners.some((corner, v) => !near(corner.ao, other.corners[map[v]].ao, AO))) {
        count(other.flags & 2 ? 'ambient occlusion' : 'ambient occlusion where Minecraft draws none', quad.state, pair);
      }
      const flat = other.corners.every((corner) => corner.sky === other.corners[0].sky && corner.block === other.corners[0].block);
      if (quad.corners.some((_, v) => !near(quad.sky, other.corners[map[v]].sky, LIGHT) || !near(quad.block, other.corners[map[v]].block, LIGHT))) {
        count(flat ? 'light (one level a face on both sides)' : 'light (smooth: Minecraft blends each corner)', quad.state, pair);
      }
    }
    // What is left: placed differently (the same facing and texture), or
    // drawn by one side only.
    const rest = [...unmatched].map((i) => theirs[i]);
    for (const quad of left) {
      const at = rest.findIndex((other) => other.direction === quad.direction && other.texture === quad.texture);
      if (at >= 0) {
        const other = rest.splice(at, 1)[0];
        count(quad.kind === 1 ? 'fluid quad placed differently' : 'model quad placed differently', quad.state, () => `mesher ${describe(quad)}\n        minecraft ${describe(other)}`);
      } else count(quad.full ? 'drawn by the mesher only (its tile at the face limit)' : `drawn by the mesher only (${quad.kind === 1 ? 'fluid' : 'model'})`, quad.state, () => `mesher ${describe(quad)}`);
    }
    for (const quad of rest) count(`drawn by Minecraft only (${quad.kind === 1 ? 'fluid' : 'model'})`, quad.state, () => `minecraft ${describe(quad)}`);
  }
  return { matched, entities, mesher: mesher.length - entities, minecraft: minecraft.quads.length };
}

// Every state of the block table, as [name, id, properties].
function tableStates(blockTable) {
  const states = [];
  for (const line of fs.readFileSync(blockTable, 'utf8').split('\n').filter(Boolean)) {
    const [name] = line.split('\t');
    if (['default', 'facehides', 'faceoccludes', 'skiprendering'].includes(name)) continue;
    const [id, properties = ''] = name.replace(/]$/, '').split('[');
    states.push([name, id, Object.fromEntries(properties ? properties.split(',').map((pair) => pair.split('=')) : [])]);
  }
  return states;
}

const STONE = 'minecraft:stone';
const STEPS = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

// The worlds of each set, as [name, () => ({ blueprint, mask })].
function cases(sets, blockTable, states) {
  const list = [];
  const facts = minecraftFacts();
  const direct = (blocks) => {
    const bytes = blocksBlueprint(blocks);
    const xs = blocks.map((b) => b[0]), zs = blocks.map((b) => b[2]);
    return { blueprint: bytes, mask: renderMask({ min: [Math.min(...xs), 0, Math.min(...zs)], max: [Math.max(...xs), 0, Math.max(...zs)] }) };
  };
  const placed = async (world) => {
    world.finalize();
    return { blueprint: await minecraftLive.placeBlueprint(null, world, null), mask: renderMask(world.bounds()) };
  };
  const names = states.map(([name]) => name);
  if (sets.has('states')) {
    // Every state alone, three blocks apart, in three places.
    const width = Math.ceil(Math.sqrt(names.length));
    for (const [label, [ox, oy, oz]] of [['states', [0, 10, 0]], ['states-moved', [1003, 77, -2011]], ['states-high', [-517, 301, 389]]]) {
      list.push([label, () => direct(names.map((name, i) => [ox + (i % width) * 3, oy, oz + Math.floor(i / width) * 3, name]))]);
    }
  }
  if (sets.has('faces')) {
    const width = Math.ceil(Math.sqrt(names.length));
    STEPS.forEach(([dx, dy, dz], d) => {
      list.push([`faces-${DIRECTIONS[d]}`, () => direct(names.flatMap((name, i) => {
        const [x, y, z] = [(i % width) * 4, 20, Math.floor(i / width) * 4];
        return [[x, y, z, name], [x + dx, y + dy, z + dz, STONE]];
      }))]);
    });
  }
  if (sets.has('liquids')) {
    // Every level of water and lava beside, above and below every level of
    // the same fluid and the other, each pair on stone apart from the rest.
    const blocks = [];
    const fluids = ['minecraft:water', 'minecraft:lava'];
    let n = 0;
    for (const a of fluids) for (const b of fluids) for (let la = 0; la < 16; la += 1) for (let lb = 0; lb < 16; lb += 1) {
      const x = (n % 64) * 4, z = Math.floor(n / 64) * 6;
      n += 1;
      blocks.push([x, 10, z, `${a}[level=${la}]`], [x + 1, 10, z, `${b}[level=${lb}]`], [x, 9, z, STONE], [x + 1, 9, z, STONE]);
      blocks.push([x, 21, z, `${a}[level=${la}]`], [x, 20, z, `${b}[level=${lb}]`]);
    }
    list.push(['liquids-pairs', () => direct(blocks)]);
    // Every waterlogged state, in a pool of still water around it, and on
    // its own.
    const logged = names.filter((name) => /waterlogged=true/.test(name));
    list.push(['liquids-waterlogged', () => {
      const pool = [];
      logged.forEach((name, i) => {
        const x = (i % 64) * 4, z = Math.floor(i / 64) * 4;
        pool.push([x, 31, z, name]);
        for (const [dx, dz] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) pool.push([x + dx, 31, z + dz, 'minecraft:water[level=0]']);
        pool.push([x, 30, z, STONE]);
        pool.push([x + 400, 31, z, name]);
      });
      return direct(pool);
    }]);
  }
  if (sets.has('arrangements')) {
    const byClass = (className) => [...new Set(states.map(([, id]) => id))].filter((id) => facts.is(id, className));
    // Runs and corners of every connecting block: a line, an L, a T and a
    // cross, beside stone and alone; the engine joins them.
    list.push(['arrangements-connecting', () => {
      const world = new World();
      const ids = [...byClass('FenceBlock'), ...byClass('WallBlock'), ...byClass('IronBarsBlock')];
      ids.forEach((id, i) => {
        const x0 = (i % 16) * 12, z0 = Math.floor(i / 16) * 12;
        for (let k = 0; k < 5; k += 1) world.set(x0 + k, 1, z0, id, {});
        for (let k = 1; k < 4; k += 1) world.set(x0, 1, z0 + k, id, {});
        for (let k = -1; k <= 1; k += 1) world.set(x0 + 6, 1, z0 + 3 + k, id, {});
        world.set(x0 + 5, 1, z0 + 3, id, {});
        world.set(x0 + 7, 1, z0 + 3, id, {});
        world.set(x0 + 9, 1, z0 + 6, id, {});
        world.set(x0 + 10, 1, z0 + 6, STONE);
        world.set(x0 + 2, 2, z0, id, {});
        for (let dx = 0; dx < 11; dx += 1) for (let dz = 0; dz < 8; dz += 1) world.set(x0 + dx, 0, z0 + dz, STONE);
      });
      return placed(world);
    }]);
    // Fence gates open and closed, in a fence and in a wall, facing each way.
    list.push(['arrangements-gates', () => {
      const world = new World();
      const gates = byClass('FenceGateBlock');
      const fence = byClass('FenceBlock')[0], wall = byClass('WallBlock')[0];
      gates.forEach((id, i) => {
        ['north', 'east', 'south', 'west'].forEach((facing, f) => ['false', 'true'].forEach((open, o) => [fence, wall, null].forEach((side, s) => {
          const x = (i % 8) * 30 + f * 7, z = Math.floor(i / 8) * 20 + o * 9 + s * 3;
          const alongX = facing === 'north' || facing === 'south';
          world.set(x + 1, 0, z + 1, STONE);
          world.set(x + 1, 1, z + 1, id, { facing, open, in_wall: 'false', powered: 'false' });
          if (side) for (const k of [-1, 1]) world.set(x + 1 + (alongX ? k : 0), 1, z + 1 + (alongX ? 0 : k), side, {});
        })));
      });
      return placed(world);
    }]);
    // Buttons and levers on every face of a stone block, every face and
    // facing they take, pressed and not.
    list.push(['arrangements-attached', () => {
      const blocks = [];
      const ids = [...byClass('ButtonBlock'), ...byClass('LeverBlock')];
      const attached = states.filter(([, id]) => ids.includes(id));
      attached.forEach(([name, , properties], i) => {
        const x = (i % 32) * 3, z = Math.floor(i / 32) * 3;
        const { face, facing } = properties;
        const step = face === 'floor' ? [0, -1, 0] : face === 'ceiling' ? [0, 1, 0] : { north: [0, 0, 1], south: [0, 0, -1], west: [1, 0, 0], east: [-1, 0, 0] }[facing];
        blocks.push([x, 41, z, name], [x + step[0], 41 + step[1], z + step[2], STONE]);
      });
      return direct(blocks);
    }]);
    // Scaffolding stacked, overhanging and bottomed out, as the engine
    // settles its distance and bottom.
    list.push(['arrangements-scaffolding', () => {
      const world = new World();
      for (const id of byClass('ScaffoldingBlock')) {
        for (let y = 0; y < 5; y += 1) world.set(0, y, 0, id, {});
        for (let x = 1; x <= 6; x += 1) world.set(x, 4, 0, id, {});
        for (let x = 1; x <= 3; x += 1) world.set(x, 0, 3, id, {});
        world.set(8, 0, 0, id, {});
        world.set(8, 1, 0, id, {});
        for (let dx = -1; dx <= 9; dx += 1) for (let dz = -1; dz <= 4; dz += 1) world.set(dx, -1, dz, STONE);
      }
      return placed(world);
    }]);
    // Hoppers facing every way, enabled and not, on stone, under and beside
    // blocks, and waterlogged-less in water.
    list.push(['arrangements-hoppers', () => {
      const blocks = [];
      const hoppers = states.filter(([, id]) => byClass('HopperBlock').includes(id));
      hoppers.forEach(([name], i) => {
        const x = i * 4;
        blocks.push([x, 50, 0, name], [x, 49, 0, STONE], [x, 50, 3, name], [x, 51, 3, STONE], [x + 1, 50, 3, STONE], [x, 50, 6, name]);
        for (const [dx, dz] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) blocks.push([x + dx, 50, 6 + dz, 'minecraft:water[level=0]']);
      });
      return direct(blocks);
    }]);
  }
  if (sets.has('fixtures')) {
    // What render-parity's fixtures are filled from: each block's states as
    // property objects, and the block table's facts.
    const table = new Map();
    for (const [, id, properties] of states) {
      if (!table.has(id)) table.set(id, []);
      table.get(id).push(properties);
    }
    const facts = new BlockStates(fs.readFileSync(blockTable, 'utf8'), fs.readFileSync(path.join(FOLDER, 'render-layers.tsv'), 'utf8'));
    for (const [name, fill] of Object.entries(FIXTURES)) {
      if (name === 'face-limit') continue;
      list.push([`fixture:${name}`, async () => {
        const world = new World();
        fill(world, table, facts);
        world.finalize();
        const mask = MASKS[name] || renderMask(world.bounds());
        return { blueprint: DIRECT.has(name) ? worldBlueprint(world, Y_OFFSET) : await minecraftLive.placeBlueprint(null, world, null), mask };
      }]);
    }
  }
  if (sets.has('builds')) {
    for (const [name, file] of corpusArtifacts().sort(([a], [b]) => a.localeCompare(b))) {
      list.push([name, () => {
        const world = new World();
        for (const block of JSON.parse(fs.readFileSync(file, 'utf8')).blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
        return placed(world);
      }]);
    }
  }
  if (sets.has('worlds')) {
    // Each chat's world as it was last saved: its manifest from the
    // database, its chunks from the worlds folder beside it.
    const database = option('worlds', null);
    if (!database) throw new Error('--worlds names a copy of a data folder\'s mcbench.sqlite3, its worlds/ folder beside it');
    const { DatabaseSync } = require('node:sqlite');
    const db = new DatabaseSync(database, { readOnly: true });
    const rows = db.prepare('select id, world_manifest_json from conversations where world_manifest_json is not null order by id').all();
    db.close();
    // --max-blocks leaves out larger worlds: the capture holds a world's
    // quads in memory, Java's and Node's (a world of 2.9 million blocks took
    // more than Java's 6 GB).
    const maxBlocks = Number(option('max-blocks', Infinity));
    for (const { id, world_manifest_json: json } of rows) {
      const manifest = JSON.parse(json);
      if (!manifest.blockCount) continue;
      if (manifest.blockCount > maxBlocks) {
        console.log(`world:${id}: left out, ${manifest.blockCount} blocks (--max-blocks ${maxBlocks})`);
        continue;
      }
      list.push([`world:${id}`, () => {
        const chunks = path.join(path.dirname(database), 'worlds', id, 'chunks');
        const world = World.fromManifest(manifest, (hash) => JSON.parse(fs.readFileSync(path.join(chunks, `${hash}.json`), 'utf8')));
        return placed(world);
      }]);
    }
  }
  return list;
}

async function main() {
  const sets = new Set(option('sets', 'states,faces,arrangements,liquids,fixtures,builds').split(','));
  const match = option('match', '');
  const limit = Number(option('limit', Infinity));
  const examples = Number(option('examples', 3));
  execFileSync('cargo', ['build', '--release', '--bin', 'mcbench-mesh'], { cwd: RENDERER, stdio: 'inherit' });
  const blockTable = await minecraftLive.blockTable();
  const states = tableStates(blockTable);
  const names = states.map(([name]) => name);
  const renderer = await start(blockTable);
  const totals = { worlds: 0, matched: 0, mesher: 0, minecraft: 0, entities: 0 };
  const tally = {};
  const results = [];
  for (const [name, make] of cases(sets, blockTable, states).filter(([name]) => name.includes(match)).slice(0, limit)) {
    const result = { name };
    try {
      const made = await make();
      if (!made) continue;
      const { mesher, minecraft, meshMs, captureMs } = await renderer.draw(made.blueprint, made.mask);
      const own = {};
      const counted = compareQuads(mesher, minecraft, names, own, examples);
      for (const [kind, entry] of Object.entries(own)) {
        const into = tally[kind] ||= { count: 0, blocks: {}, examples: [], worlds: 0 };
        into.count += entry.count;
        into.worlds += 1;
        for (const [block, n] of Object.entries(entry.blocks)) into.blocks[block] = (into.blocks[block] || 0) + n;
        for (const example of entry.examples) if (into.examples.length < examples) into.examples.push(`${name}: ${example}`);
      }
      Object.assign(result, counted, { meshMs, captureMs, differences: Object.fromEntries(Object.entries(own).map(([kind, entry]) => [kind, entry.count])) });
      totals.worlds += 1;
      for (const key of ['matched', 'mesher', 'minecraft', 'entities']) totals[key] += counted[key];
      console.log(`${name}: ${counted.mesher} mesher quads, ${counted.minecraft} Minecraft's, ${counted.matched} matched, ${counted.entities} block entity quads apart; `
        + `${Object.entries(result.differences).map(([kind, n]) => `${kind} ${n}`).join(', ') || 'no differences'} (mesh ${meshMs.toFixed(0)} ms, Minecraft ${captureMs.toFixed(0)} ms)`);
    } catch (error) {
      result.error = String(error.stack || error).slice(0, 2000);
      console.log(`${name}: error: ${result.error}`);
    }
    results.push(result);
  }
  renderer.stop();
  await minecraftLive.engine.stop();
  console.log(`\n${totals.worlds} worlds: ${totals.mesher} mesher quads, ${totals.minecraft} Minecraft's, ${totals.matched} matched, ${totals.entities} block entity quads apart`);
  for (const [kind, entry] of Object.entries(tally).sort(([, a], [, b]) => b.count - a.count)) {
    const top = Object.entries(entry.blocks).sort(([, a], [, b]) => b - a);
    console.log(`  ${kind}: ${entry.count} quads in ${entry.worlds} worlds, ${top.length} blocks: ${top.slice(0, 12).map(([block, n]) => `${block.replace('minecraft:', '')} ${n}`).join(', ')}`);
    for (const example of entry.examples) console.log(`      ${example}`);
  }
  const report = option('report', null);
  if (report) fs.writeFileSync(report, JSON.stringify({ totals, tally, results }, null, 2));
  const errored = results.filter((result) => result.error).length;
  return !errored && !Object.keys(tally).length;
}

main().then((passed) => process.exit(passed ? 0 : 1), (error) => { console.error(error); process.exit(1); });
