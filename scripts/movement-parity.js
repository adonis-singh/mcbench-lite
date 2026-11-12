'use strict';

// Checks the Rust port of the creative-mode player's movement (core/src/player)
// against Mojang's own 1.21.10 client (scripts/MovementReference.java, see
// scripts/movement-ref.js) bit for bit: both get the same worlds (blocks set
// without any update), the same player and the same input sequences, and every
// tick's position, velocity, onGround, flying, pose, sprinting, crouching,
// collisions and fall distance must be identical. A case the port refuses
// ("unported: ...") counts as refused, not as a match.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/movement-parity.js
//     [--fuzz N] [--seed S] [--ticks T]    random worlds (default 300 worlds)
//     [--builds regex] [--limit N] [--players P] [--saved]   the builds of the corpus MCBENCH_CORPUS names (with --saved, the builds saved in the
//                                          data folder) as the Java engine places them, P players in each
//     [--states] [--match regex]           every state of every block (of the blocks that match), a player in it and on it
//     [--pistons N]                        pistons of each build moving, 4 by default
//     [--cases file]                       a cases file as it is (see MovementReference.java)
//     [--keep folder]                      keep the cases and both traces there
//     [--autojump]                         the auto-jump option on in every case

const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { prepare } = require('./movement-ref');
const { jvmEngine, rust } = require('../server/engine');
const { minecraftLive } = require('../server/minecraft-live');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');

const ROOT = path.join(__dirname, '..');
const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

// --- The registry the generator draws blocks from.
function loadRegistry() {
  const blocks = [];
  let block = null;
  require('../server/minecraft-tables').checkTables('1.21.10');
  for (const line of fs.readFileSync(path.join(ROOT, 'core', 'minecraft', '1.21.10', 'blocks.txt'), 'utf8').split('\n')) {
    const p = line.split('\t');
    if (p[0] === 'block') {
      block = { key: p[1], count: Number(p[5]), properties: [] };
      blocks.push(block);
    } else if (p[0] === 'property' && block) {
      block.properties.push({ name: p[1], values: p[2].split(',') });
    }
  }
  const byKey = new Map(blocks.map((b) => [b.key, b]));
  const stateOf = (b, offset) => {
    if (!b.properties.length) return b.key;
    let rest = offset;
    const parts = [];
    for (let i = b.properties.length - 1; i >= 0; i -= 1) {
      const n = b.properties[i].values.length;
      parts.unshift(`${b.properties[i].name}=${b.properties[i].values[rest % n]}`);
      rest = Math.floor(rest / n);
    }
    return `${b.key}[${parts.join(',')}]`;
  };
  return { blocks, byKey, stateOf };
}

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Blocks the port does not read from a raw world: a moving piston's shape is
// its block entity's, which a raw world does not have.
const EXCLUDED = new Set(['minecraft:moving_piston']);

// --- A random world, player and input sequence.
function generate(rng, registry, name, { ticks, autojump }) {
  const int = (n) => Math.floor(rng() * n);
  const range = (a, b) => a + int(b - a + 1);
  const pick = (list) => list[int(list.length)];
  const chance = (p) => rng() < p;
  const lines = [`case ${name}`];
  const candidates = registry.blocks.filter((b) => !EXCLUDED.has(b.key) && b.key !== 'minecraft:air' && b.key !== 'minecraft:cave_air' && b.key !== 'minecraft:void_air');
  const randomState = (b) => registry.stateOf(b, int(b.count));
  const named = (key) => registry.byKey.get(`minecraft:${key}`);
  // The world sits at an offset from the origin now and then, to cross chunks and
  // the negative coordinates the offsets of bamboo and the like are drawn from.
  const ox = chance(0.5) ? 0 : chance(0.5) ? 16 * range(-100, 100) : range(-5000, 5000);
  const oz = chance(0.5) ? 0 : chance(0.5) ? 16 * range(-100, 100) : range(-5000, 5000);
  const set = (x, y, z, state) => lines.push(`set ${x + ox} ${y} ${z + oz} ${state}`);
  const fill = (x1, y1, z1, x2, y2, z2, state) => lines.push(`fill ${x1 + ox} ${y1} ${z1 + oz} ${x2 + ox} ${y2} ${z2 + oz} ${state}`);
  const R = 10;

  // The floor.
  const floorBlocks = ['stone', 'ice', 'packed_ice', 'blue_ice', 'slime_block', 'soul_sand', 'honey_block', 'sand', 'oak_planks', 'dirt_path', 'farmland[moisture=0]', 'mud'];
  fill(-R, -4, -R, R, 0, R, 'minecraft:stone');
  if (chance(0.6)) {
    for (let i = range(1, 4); i > 0; i -= 1) {
      const x = range(-R, R);
      const z = range(-R, R);
      fill(x, 0, z, Math.min(R, x + range(1, 8)), 0, Math.min(R, z + range(1, 8)), `minecraft:${pick(floorBlocks)}`);
    }
  }
  const features = range(3, 14);
  for (let f = 0; f < features; f += 1) {
    const x = range(-R, R);
    const z = range(-R, R);
    const kind = int(14);
    if (chance(0.04)) set(x, 1, z, 'minecraft:stone');
    if (kind === 0) {
      for (let i = range(1, 25); i > 0; i -= 1) {
        set(range(-R, R), range(1, 4), range(-R, R), randomState(pick(candidates)));
      }
    } else if (kind === 1) {
      const b = pick(candidates);
      for (let y = 1; y <= range(1, 4); y += 1) set(x, y, z, randomState(b));
    } else if (kind === 2) {
      // Steps up in a direction, of one block kind.
      const dx = pick([-1, 1, 0, 0]);
      const dz = dx ? 0 : pick([-1, 1]);
      const b = pick([named('stone'), named('oak_slab'), named('oak_stairs'), named('snow'), named('oak_trapdoor'), pick(candidates)]);
      for (let i = 0; i < range(2, 6); i += 1) {
        for (let w = 0; w < range(1, 3); w += 1) set(x + dx * i + (dx ? 0 : w), range(1, 1) + Math.floor(i / 2), z + dz * i + (dz ? 0 : w), randomState(b));
      }
    } else if (kind === 3) {
      // A pool of water or lava, sunk in the floor, sometimes with flowing edges.
      const liquid = chance(0.7) ? 'water' : 'lava';
      const depth = range(1, 3);
      const x2 = Math.min(R, x + range(1, 6));
      const z2 = Math.min(R, z + range(1, 6));
      fill(x, -depth + 1, z, x2, 0, z2, `minecraft:${liquid}[level=0]`);
      if (chance(0.5)) fill(x, 1, z, x2, range(1, 3), z2, `minecraft:${liquid}[level=0]`);
      if (chance(0.5)) set(x2 + 1, 0, z, `minecraft:${liquid}[level=${range(1, 7)}]`);
      if (chance(0.3)) set(x, 1, z - 1, `minecraft:${liquid}[level=${range(8, 15)}]`);
    } else if (kind === 4) {
      // A climbable column against a wall.
      const key = pick(['ladder', 'vine', 'scaffolding', 'twisting_vines', 'weeping_vines', 'cave_vines', 'oak_trapdoor']);
      const height = range(2, 8);
      fill(x, 1, z, x, height, z, 'minecraft:stone');
      for (let y = 1; y <= height; y += 1) set(x + 1, y, z, randomState(named(key)));
    } else if (kind === 5) {
      for (let dx = 0; dx < 3; dx += 1) for (let dy = 0; dy < 3; dy += 1) for (let dz = 0; dz < 3; dz += 1) if (chance(0.7)) set(x + dx, 1 + dy, z + dz, 'minecraft:cobweb');
    } else if (kind === 6) {
      // A bubble column over soul sand or magma.
      const base = pick(['soul_sand', 'magma_block']);
      const height = range(2, 7);
      const drag = base === 'magma_block';
      fill(x, 0, z, x, 0, z, `minecraft:${base}`);
      fill(x - 1, 1, z - 1, x + 1, height, z + 1, 'minecraft:water[level=0]');
      fill(x, 1, z, x, height, z, `minecraft:bubble_column[drag=${drag}]`);
    } else if (kind === 7) {
      // A low tunnel.
      const x2 = Math.min(R, x + range(2, 8));
      const z2 = Math.min(R, z + range(2, 8));
      fill(x, range(2, 3), z, x2, 3, z2, pick(['minecraft:stone', 'minecraft:glass', 'minecraft:oak_slab[type=bottom,waterlogged=false]', 'minecraft:oak_trapdoor[open=false,half=bottom,facing=north,powered=false,waterlogged=false]']));
    } else if (kind === 8) {
      const b = pick(['honey_block', 'slime_block', 'powder_snow', 'sweet_berry_bush', 'soul_sand', 'cobweb']);
      for (let i = 0; i < range(1, 6); i += 1) set(x + int(3), 1 + int(2), z + int(3), randomState(named(b)));
    } else if (kind === 9) {
      const x2 = Math.min(R, x + range(1, 6));
      fill(x, 0, z, x2, 0, Math.min(R, z + range(1, 5)), pick(['minecraft:ice', 'minecraft:blue_ice', 'minecraft:packed_ice']));
    } else if (kind === 10) {
      const b = pick(['oak_fence', 'cobblestone_wall', 'oak_fence_gate', 'iron_bars', 'glass_pane']);
      for (let i = 0; i < range(2, 7); i += 1) set(x + i, 1, z, randomState(named(b)));
    } else if (kind === 11) {
      const b = pick(['snow', 'white_carpet', 'oak_slab', 'oak_stairs', 'white_bed', 'cake', 'hopper', 'composter', 'cauldron', 'anvil', 'chest', 'lantern', 'end_rod', 'flower_pot', 'campfire', 'bell']);
      for (let i = 0; i < range(1, 8); i += 1) set(x + int(4), 1, z + int(4), randomState(named(b)));
    } else if (kind === 12) {
      // A hollow room.
      const x2 = Math.min(R, x + range(2, 5));
      const z2 = Math.min(R, z + range(2, 5));
      const h = range(2, 4);
      fill(x, 1, z, x2, h, z2, 'minecraft:stone');
      fill(x + 1, 1, z + 1, x2 - 1, h - 1, z2 - 1, 'minecraft:air');
      set(x + 1, 1, z, 'minecraft:air');
    } else {
      // A wall of one random kind.
      const b = pick(candidates);
      for (let i = 0; i < range(2, 8); i += 1) for (let y = 1; y <= range(1, 2); y += 1) set(x + i, y, z, randomState(b));
    }
  }

  // A feature of one mechanic at the middle, where the player starts nearby.
  const family = pick(['none', 'ladder', 'web', 'berry', 'powder', 'pool', 'lava', 'ice', 'slime', 'honey', 'soulsand', 'bed', 'stairs', 'edge', 'crawl', 'bubble', 'scaffold', 'fence', 'trapdoor', 'snow']);
  const half = range(2, 5);
  const around = (state, p = 0.5) => {
    for (let x = -half; x <= half; x += 1) for (let z = -half; z <= half; z += 1) if (chance(p)) set(x, 1, z, state);
  };
  if (family === 'ladder') {
    const facing = pick(['north', 'south', 'east', 'west']);
    const [dx, dz] = { north: [0, 1], south: [0, -1], east: [-1, 0], west: [1, 0] }[facing];
    const h = range(3, 9);
    fill(dx * 1, 1, dz * 1, dx * 1, h, dz * 1, 'minecraft:stone');
    for (let y = 1; y <= h; y += 1) set(0, y, 0, pick([`minecraft:ladder[facing=${facing},waterlogged=false]`, 'minecraft:vine[up=false,north=false,south=false,east=false,west=false]'.replace(`${facing === 'north' ? 'south' : facing === 'south' ? 'north' : facing === 'east' ? 'west' : 'east'}=false`, `${facing === 'north' ? 'south' : facing === 'south' ? 'north' : facing === 'east' ? 'west' : 'east'}=true`), 'minecraft:scaffolding[bottom=false,distance=0,waterlogged=false]']));
  } else if (family === 'web') {
    for (let x = -1; x <= 1; x += 1) for (let y = 1; y <= 4; y += 1) for (let z = -1; z <= 1; z += 1) if (chance(0.6)) set(x, y, z, 'minecraft:cobweb');
  } else if (family === 'berry') {
    around(`minecraft:sweet_berry_bush[age=${int(4)}]`, 0.5);
  } else if (family === 'powder') {
    const depth = range(1, 3);
    fill(-half, -depth + 1, -half, half, 0, half, 'minecraft:powder_snow');
  } else if (family === 'pool' || family === 'lava') {
    const liquid = family === 'pool' ? 'water' : 'lava';
    const depth = range(1, 3);
    fill(-half, -depth + 1, -half, half, 0, half, `minecraft:${liquid}[level=0]`);
    if (chance(0.5)) fill(-half, 1, -half, half, range(1, 3), half, `minecraft:${liquid}[level=0]`);
    if (chance(0.5)) fill(-1, 1, -1, 1, range(2, 4), 1, 'minecraft:stone');
    // A stream: flowing liquid of every level across the floor, some falling.
    if (chance(0.5)) {
      for (let x = -half - 2; x <= half + 2; x += 1) for (let z = -half - 2; z <= half + 2; z += 1) if (chance(0.35)) set(x, 1, z, `minecraft:${liquid}[level=${chance(0.25) ? range(8, 15) : range(1, 7)}]`);
    }
  } else if (family === 'ice') {
    fill(-half, 0, -half, half, 0, half, pick(['minecraft:ice', 'minecraft:packed_ice', 'minecraft:blue_ice']));
    around('minecraft:stone', 0.1);
  } else if (family === 'slime') {
    fill(-half, 0, -half, half, 0, half, 'minecraft:slime_block');
    around('minecraft:stone', 0.1);
  } else if (family === 'honey') {
    fill(-half, 0, -half, half, 0, half, 'minecraft:honey_block');
    fill(half, 1, -half, half, 4, half, 'minecraft:honey_block');
  } else if (family === 'soulsand') {
    fill(-half, 0, -half, half, 0, half, 'minecraft:soul_sand');
  } else if (family === 'bed') {
    for (let x = -half; x <= half; x += 2) for (let z = -half; z <= half; z += 2) if (chance(0.6)) {
      set(x, 1, z, 'minecraft:red_bed[facing=north,occupied=false,part=foot]');
      set(x, 1, z - 1, 'minecraft:red_bed[facing=north,occupied=false,part=head]');
    }
  } else if (family === 'stairs') {
    const dir = pick(['north', 'south', 'east', 'west']);
    const kind = pick(['oak_stairs', 'stone', 'oak_slab[type=bottom,waterlogged=false]', 'cobblestone_stairs']);
    for (let i = 0; i < range(3, 7); i += 1) fill(-half + i * 1, 1, -half, -half + i * 1, 1 + Math.floor(i / 2), half, kind.includes('stairs') ? `minecraft:${kind}[facing=east,half=bottom,shape=straight,waterlogged=false]` : `minecraft:${kind}`);
  } else if (family === 'edge') {
    // A platform at height with open edges.
    fill(-half, 0, -half, half, 0, half, 'minecraft:air');
    fill(-half, -4, -half, half, -4, half, 'minecraft:stone');
    fill(-1, 3, -1, 1, 3, 1, 'minecraft:stone');
    fill(-half, 0, -half, half, 0, half, chance(0.5) ? 'minecraft:air' : 'minecraft:stone');
    fill(-half, 1, -half, half, 1, -half, 'minecraft:oak_slab[type=bottom,waterlogged=false]');
  } else if (family === 'crawl') {
    fill(-half, 2, -half, half, 2, half, pick(['minecraft:stone', 'minecraft:oak_trapdoor[open=false,half=bottom,facing=north,powered=false,waterlogged=false]']));
    if (chance(0.5)) fill(-half, 1, -half, half, 1, half, 'minecraft:air');
    fill(-half, 1, -half, half, 1, -half, 'minecraft:stone');
  } else if (family === 'bubble') {
    const base = pick(['soul_sand', 'magma_block']);
    fill(0, 0, 0, 0, 0, 0, `minecraft:${base}`);
    fill(-half, 1, -half, half, range(3, 8), half, 'minecraft:water[level=0]');
    fill(0, 1, 0, 0, range(3, 8), 0, `minecraft:bubble_column[drag=${base === 'magma_block'}]`);
  } else if (family === 'scaffold') {
    for (let x = -half; x <= half; x += 1) for (let z = -half; z <= half; z += 1) if (chance(0.5)) for (let y = 1; y <= range(1, 5); y += 1) set(x, y, z, `minecraft:scaffolding[bottom=${chance(0.4)},distance=${int(8)},waterlogged=false]`);
  } else if (family === 'fence') {
    around(pick(['minecraft:oak_fence', 'minecraft:cobblestone_wall', 'minecraft:oak_fence_gate[facing=north,in_wall=false,open=false,powered=false]']), 0.4);
  } else if (family === 'trapdoor') {
    const facing = pick(['north', 'south', 'east', 'west']);
    for (let y = 1; y <= 4; y += 1) set(0, y, 0, y === 1 ? `minecraft:ladder[facing=${facing},waterlogged=false]` : `minecraft:oak_trapdoor[open=true,half=bottom,facing=${facing},powered=false,waterlogged=false]`);
    fill(1, 1, 0, 1, 4, 0, 'minecraft:stone');
  } else if (family === 'snow') {
    for (let x = -half; x <= half; x += 1) for (let z = -half; z <= half; z += 1) if (chance(0.7)) set(x, 1, z, `minecraft:snow[layers=${range(1, 8)}]`);
  }

  // The player.
  const near = family !== 'none' && chance(0.85);
  const px = near ? (rng() * 2 - 1) * (half + 1) + 0.5 : (rng() * 2 - 1) * (R - 2) + 0.5;
  const pz = near ? (rng() * 2 - 1) * (half + 1) + 0.5 : (rng() * 2 - 1) * (R - 2) + 0.5;
  // Now and then the player starts high (a long fall, the build's top).
  const py = chance(0.1) ? 5 + rng() * 10 : chance(0.06) ? 20 + rng() * 200 : chance(0.02) ? 900 + rng() * 60 : chance(0.02) ? -100 - rng() * 100 : 1 + rng() * (near ? 3 : 5);
  // Boats standing about, which a player collides with.
  if (chance(0.3)) {
    for (let i = range(1, 4); i > 0; i -= 1) lines.push(`boat ${Math.floor(px + ox) + range(-4, 4) + rng()} ${chance(0.7) ? 1 : range(0, 3) + rng()} ${Math.floor(pz + oz) + range(-4, 4) + rng()}`);
  }
  // Blocks that appear and vanish around the player while it moves.
  const editing = chance(0.4);
  const edit = (t) => {
    if (!editing || !chance(0.08)) return [];
    const state = chance(0.4) ? 'minecraft:air' : randomState(pick(candidates));
    // At the player's block now and then, which its cached block (inBlockState) must not outlive.
    if (chance(0.5)) {
      const now = chance(0.6) ? pick(['minecraft:air', 'minecraft:ladder[facing=north,waterlogged=false]', 'minecraft:water[level=0]', 'minecraft:cobweb', 'minecraft:vine[up=true,north=false,south=false,east=false,west=false]', 'minecraft:stone', 'minecraft:oak_slab[type=bottom,waterlogged=false]', 'minecraft:powder_snow']) : state;
      return [`editplayer ${range(-1, 1)} ${range(-1, 2)} ${range(-1, 1)} ${now}`];
    }
    return [`edit ${Math.floor(px + ox) + range(-4, 4)} ${range(0, 4)} ${Math.floor(pz + oz) + range(-4, 4)} ${state}`];
  };
  lines.push(...playerScript(rng, { x: px + ox, y: py, z: pz + oz, flying: chance(0.12), ticks, autojump, edit }));
  lines.push('end');
  lines[0] = `case ${name}-${family}`;
  return lines.join('\n');
}


// --- Pistons. A moving piston is block entity data (what a piston leaves
// where a block moves), so a scene is made of the commands that give both
// sides the block and its block entity at progress 0, as a piston extending
// or retracting makes them (PistonBaseBlock.moveBlocks).
const DIRECTIONS = { north: [0, 0, -1], south: [0, 0, 1], west: [-1, 0, 0], east: [1, 0, 0], up: [0, 1, 0], down: [0, -1, 0] };

// The commands of a piston at [bx, by, bz] facing `dir` that extends (pushing
// `pushed`, block states in the order from the piston) or retracts (a sticky
// piston pulling `pushed`).
function pistonScene([bx, by, bz], dir, kind, sticky, pushed) {
  const [dx, dy, dz] = DIRECTIONS[dir];
  const at = (i) => `${bx + dx * i} ${by + dy * i} ${bz + dz * i}`;
  const type = sticky ? 'sticky' : 'normal';
  const key = sticky ? 'minecraft:sticky_piston' : 'minecraft:piston';
  const moving = `minecraft:moving_piston[facing=${dir},type=${type}]`;
  const lines = [];
  if (kind === 'extend') {
    lines.push(`set ${bx} ${by} ${bz} ${key}[extended=true,facing=${dir}]`);
    pushed.forEach((state, i) => lines.push(`moving ${at(i + 2)} ${moving} ${state} ${dir} 1 0`));
    lines.push(`moving ${at(1)} ${moving} minecraft:piston_head[facing=${dir},short=false,type=${type}] ${dir} 1 1`);
  } else {
    lines.push(`set ${at(1)} minecraft:air`);
    pushed.forEach((state, i) => lines.push(`moving ${at(i + 1)} ${moving} ${state} ${dir} 0 0`));
    lines.push(`moving ${bx} ${by} ${bz} ${moving} ${key}[extended=false,facing=${dir}] ${dir} 0 1`);
  }
  return lines;
}

// Where a player starts for a piston scene: mostly in the way of the head and
// the blocks it moves, standing on or beside them.
function pistonSpawn(rng, [bx, by, bz], [dx, dy, dz], n) {
  if (rng() < 0.7) {
    const k = 1 + Math.floor(rng() * (n + 1));
    const top = rng() < 0.5;
    return [bx + dx * k + 0.5 + (rng() - 0.5) * 0.9, by + dy * k + (top ? 1.0 + rng() * 0.2 : rng() * 0.9), bz + dz * k + 0.5 + (rng() - 0.5) * 0.9];
  }
  const along = rng() * (n + 3);
  return [bx + 0.5 + dx * along + (rng() - 0.5) * 3, by + 0.2 + dy * along + rng() * 2.5, bz + 0.5 + dz * along + (rng() - 0.5) * 3];
}

// A world of clean blocks (nothing that a finished piston's updates would
// reshape) with pistons, and a player in their way.
function generatePiston(rng, registry, name, { ticks, autojump }) {
  const int = (n) => Math.floor(rng() * n);
  const range = (a, b) => a + int(b - a + 1);
  const pick = (list) => list[int(list.length)];
  const chance = (p) => rng() < p;
  const R = 10;
  const ox = chance(0.5) ? 0 : range(-5000, 5000);
  const oz = chance(0.5) ? 0 : range(-5000, 5000);
  const lines = [`case ${name}-piston`];
  lines.push(`fill ${-R + ox} -4 ${-R + oz} ${R + ox} 0 ${R + oz} ${pick(['minecraft:stone', 'minecraft:stone', 'minecraft:ice', 'minecraft:slime_block', 'minecraft:honey_block'])}`);
  const clean = ['minecraft:stone', 'minecraft:slime_block', 'minecraft:honey_block', 'minecraft:glass', 'minecraft:oak_planks', 'minecraft:oak_slab[type=bottom,waterlogged=false]', 'minecraft:oak_slab[type=top,waterlogged=false]', 'minecraft:ice', 'minecraft:soul_sand'];
  for (let i = range(0, 6); i > 0; i -= 1) lines.push(`set ${range(-R, R) + ox} ${range(1, 4)} ${range(-R, R) + oz} ${pick(clean)}`);
  const scenes = chance(0.3) ? 2 : 1;
  const spots = [];
  for (let scene = 0; scene < scenes; scene += 1) {
    const dir = pick(Object.keys(DIRECTIONS));
    const sticky = chance(0.5);
    const kind = pick(['extend', 'retract']);
    const base = [range(-3, 3) + ox + scene * 7, range(2, 4), range(-3, 3) + oz];
    const pushed = Array.from({ length: kind === 'retract' && !sticky ? 0 : range(0, 4) }, () => pick(clean));
    // The head and the blocks it moves go in front of the piston; clear the way.
    const [dx, dy, dz] = DIRECTIONS[dir];
    const end = [base[0] + dx * 6, base[1] + dy * 6, base[2] + dz * 6];
    lines.push(`fill ${Math.min(base[0], end[0])} ${Math.min(base[1], end[1])} ${Math.min(base[2], end[2])} ${Math.max(base[0], end[0])} ${Math.max(base[1], end[1])} ${Math.max(base[2], end[2])} minecraft:air`);
    lines.push(...pistonScene(base, dir, kind, sticky, pushed));
    spots.push({ base, d: [dx, dy, dz], n: pushed.length });
  }
  const spot = pick(spots);
  const [sx, sy, sz] = pistonSpawn(rng, spot.base, spot.d, spot.n);
  lines.push(
    ...playerScript(rng, {
      x: sx,
      y: sy,
      z: sz,
      flying: chance(0.1),
      ticks,
      autojump,
      idle: chance(0.6) ? range(2, 5) : 0,
    }),
  );
  lines.push('end');
  return lines.join('\n');
}

// Pistons of a build, extending in front of themselves with the blocks there
// pushed along: [position, direction, sticky, the blocks in the way].
function buildPistons(blueprint) {
  const { palette, sections } = blueprint;
  const pistons = new Map();
  palette.forEach((state, index) => {
    const match = state.match(/^minecraft:(sticky_)?piston\[(.*)\]$/);
    if (!match || !match[2].includes('extended=false')) return;
    pistons.set(index, { sticky: Boolean(match[1]), dir: match[2].match(/facing=(\w+)/)[1] });
  });
  const blocks = new Map();
  const found = [];
  for (const { sx, sy, sz, values } of sections) {
    for (let at = 0; at < 4096; at += 1) {
      if (!values[at]) continue;
      const x = sx * 16 + (at & 15);
      const y = sy * 16 + (at >> 8);
      const z = sz * 16 + ((at >> 4) & 15);
      blocks.set(`${x},${y},${z}`, values[at]);
      if (pistons.has(values[at])) found.push([x, y, z, pistons.get(values[at])]);
    }
  }
  return found.map(([x, y, z, piston]) => {
    const [dx, dy, dz] = DIRECTIONS[piston.dir];
    const pushed = [];
    for (let i = 1; i <= 4; i += 1) {
      const index = blocks.get(`${x + dx * i},${y + dy * i},${z + dz * i}`);
      if (index === undefined || palette[index].includes('piston')) break;
      pushed.push(palette[index]);
    }
    return { base: [x, y, z], dir: piston.dir, sticky: piston.sticky, pushed };
  });
}

// The build with a piston of it moving (the player has the 3 ticks before the
// finished piston's block updates reach the blocks around it).
function buildPistonCase(rng, name, blueprint, piston, { autojump }) {
  const chance = (p) => rng() < p;
  const kind = chance(0.7) ? 'extend' : 'retract';
  const lines = [`case ${name}`, ...blueprintCommands(blueprint)];
  const [dx, dy, dz] = DIRECTIONS[piston.dir];
  const [bx, by, bz] = piston.base;
  lines.push(...pistonScene(piston.base, piston.dir, kind, piston.sticky, kind === 'retract' && !piston.sticky ? [] : piston.pushed));
  const [sx, sy, sz] = pistonSpawn(rng, piston.base, [dx, dy, dz], piston.pushed.length);
  lines.push(
    ...playerScript(rng, {
      x: sx,
      y: sy,
      z: sz,
      flying: chance(0.1),
      ticks: 3,
      autojump,
      idle: chance(0.6) ? 2 : 0,
    }),
  );
  lines.push('end');
  return lines.join('\n');
}

// A player at a position and the keys it holds for `ticks` ticks: held for
// stretches, with taps between.
function playerScript(rng, { x, y, z, flying, ticks, autojump, edit = () => [], idle = 0 }) {
  const int = (n) => Math.floor(rng() * n);
  const range = (a, b) => a + int(b - a + 1);
  const chance = (p) => rng() < p;
  const yaw = Math.fround((rng() * 2 - 1) * 540);
  const lines = [`player ${x} ${y} ${z} ${yaw} ${Math.fround((rng() * 2 - 1) * 30)} ${flying ? 1 : 0}${autojump ? ' 1' : ''}`];
  let bits = 0;
  let currentYaw = yaw;
  let currentPitch = 0;
  let hold = 0;
  const FORWARD = 1;
  const BACK = 2;
  const LEFT = 4;
  const RIGHT = 8;
  const JUMP = 16;
  const SHIFT = 32;
  const SPRINT = 64;
  let jumpQueue = [];
  for (let t = 0; t < ticks; t += 1) {
    if (hold <= 0) {
      hold = range(1, 25);
      bits = 0;
      if (chance(0.7)) bits |= FORWARD;
      if (chance(0.08)) bits |= BACK;
      if (chance(0.15)) bits |= LEFT;
      if (chance(0.15)) bits |= RIGHT;
      if (chance(0.25)) bits |= SHIFT;
      if (chance(0.3)) bits |= SPRINT;
      if (chance(0.25)) bits |= JUMP;
      // A double tap on jump.
      if (chance(0.1)) jumpQueue = [JUMP, 0, JUMP];
    }
    hold -= 1;
    let current = bits;
    if (jumpQueue.length) current = (current & ~JUMP) | jumpQueue.shift();
    // A tap on forward (the double-tap that sprints).
    if (chance(0.03)) current ^= FORWARD;
    if (chance(0.04)) currentYaw = Math.fround(currentYaw + (rng() * 2 - 1) * 90);
    else if (chance(0.3)) currentYaw = Math.fround(currentYaw + (rng() * 2 - 1) * 4);
    if (chance(0.05)) currentPitch = Math.fround(Math.max(-90, Math.min(90, currentPitch + (rng() * 2 - 1) * 40)));
    lines.push(...edit(t));
    // (An idle player stands where it was put.)
    if (t < idle) current = 0;
    lines.push(`tick ${current} ${currentYaw} ${currentPitch}`);
  }
  return lines;
}

// --- Worlds the Java engine places: the community builds of the corpus MCBENCH_CORPUS names,
// placed as the live import places them, read back from the blueprint the
// engine writes (McbenchEngine.write: the palette, the bounds, then the
// sections' 4096 states each).
function readBlueprint(bytes) {
  let at = 0;
  const int = () => { const v = bytes.readInt32BE(at); at += 4; return v; };
  const short = () => { const v = bytes.readUInt16BE(at); at += 2; return v; };
  const utf = () => { const n = short(); const v = bytes.toString('utf8', at, at + n); at += n; return v; };
  const palette = [];
  for (let n = int(); n > 0; n -= 1) {
    const key = utf();
    const properties = [];
    for (let p = int(); p > 0; p -= 1) properties.push(`${utf()}=${utf()}`);
    palette.push(properties.length ? `${key}[${properties.join(',')}]` : key);
  }
  const bounds = [int(), int(), int(), int(), int(), int()];
  const sections = [];
  for (let n = int(); n > 0; n -= 1) {
    const [sx, sy, sz] = [int(), int(), int()];
    const values = new Uint16Array(4096);
    for (let i = 0; i < 4096; i += 1) values[i] = short();
    sections.push({ sx, sy, sz, values });
  }
  return { palette, bounds, sections };
}

// The blueprint as commands: runs of one state along x become a fill.
function blueprintCommands({ palette, sections }) {
  const rows = new Map();
  for (const { sx, sy, sz, values } of sections) {
    for (let at = 0; at < 4096; at += 1) {
      if (!values[at]) continue;
      const key = `${sy * 16 + (at >> 8)} ${sz * 16 + ((at >> 4) & 15)}`;
      if (!rows.has(key)) rows.set(key, []);
      rows.get(key).push([sx * 16 + (at & 15), values[at]]);
    }
  }
  const lines = [];
  for (const [key, cells] of rows) {
    cells.sort((a, b) => a[0] - b[0]);
    const [y, z] = key.split(' ');
    let i = 0;
    while (i < cells.length) {
      let j = i;
      while (j + 1 < cells.length && cells[j + 1][1] === cells[i][1] && cells[j + 1][0] === cells[j][0] + 1) j += 1;
      lines.push(`fill ${cells[i][0]} ${y} ${z} ${cells[j][0]} ${y} ${z} ${palette[cells[i][1]]}`);
      i = j + 1;
    }
  }
  return lines;
}

// The builds saved in the data folder (the conversations' worlds): [name, load()].
function savedBuilds(dataDir) {
  const Database = require('better-sqlite3');
  const database = new Database(path.join(dataDir, 'mcbench.sqlite3'), { readonly: true, fileMustExist: true });
  const rows = database.prepare('SELECT id, world_manifest_json FROM conversations').all();
  database.close();
  const builds = [];
  for (const row of rows) {
    const manifest = JSON.parse(row.world_manifest_json);
    if (!manifest.blockCount) continue;
    builds.push([`saved-${row.id.slice(0, 8)}`, () => World.fromManifest(manifest, (hash) => JSON.parse(fs.readFileSync(path.join(dataDir, 'worlds', row.id, 'chunks', `${hash}.json`), 'utf8')))]);
  }
  return builds;
}

function communityBuilds() {
  const builds = corpusArtifacts();
  return builds.sort(([a], [b]) => a.localeCompare(b));
}

// The Java engine's worlds for the builds that match: [name, blueprint].
async function placedBuilds(scratch, pattern, limit, saved = null) {
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  fs.mkdirSync(path.join(scratch, 'classes'));
  const engine = jvmEngine({ classpath, classes: path.join(scratch, 'classes') });
  await engine.warm({ classpath, binaries: path.join(scratch, 'binaries'), folder: path.join(scratch, 'java'), minecraftVersion: minecraftLive.minecraftVersion });
  const placed = [];
  try {
    const builds = saved
      ? savedBuilds(saved)
      : communityBuilds().map(([name, file]) => [name, () => {
        const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
        const world = new World();
        for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
        world.finalize();
        return world;
      }]);
    for (const [name, load] of builds.filter(([name]) => pattern.test(name)).slice(0, limit)) {
      const world = load();
      const commands = path.join(scratch, 'commands.txt');
      const blueprint = path.join(scratch, 'blueprint.bin');
      fs.writeFileSync(commands, `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
      await engine.place(commands, blueprint, { fresh: true });
      placed.push([name, readBlueprint(fs.readFileSync(blueprint))]);
    }
  } finally {
    await engine.stop();
  }
  return placed;
}

// Players placed in a build: in it, above it and around it.
function buildCase(rng, name, blueprint, { players, ticks, autojump }) {
  const chance = (p) => rng() < p;
  const [x1, y1, z1, x2, y2, z2] = blueprint.bounds;
  const lines = [`case ${name}`, ...blueprintCommands(blueprint)];
  for (let p = 0; p < players; p += 1) {
    lines.push(
      ...playerScript(rng, {
        x: x1 - 3 + rng() * (x2 - x1 + 7),
        y: y1 + rng() * (y2 - y1 + 6),
        z: z1 - 3 + rng() * (z2 - z1 + 7),
        flying: chance(0.2),
        ticks,
        autojump,
      }),
    );
  }
  lines.push('end');
  return lines.join('\n');
}

// Every state of every block, a player dropped into it and a player standing
// where it is the floor: the whole registry in cases of `per` states each.
function stateCases(registry, pattern, per) {
  const cells = [];
  for (const block of registry.blocks) {
    if (block.key === 'minecraft:moving_piston' || !pattern.test(block.key)) continue;
    for (let offset = 0; offset < block.count; offset += 1) cells.push(registry.stateOf(block, offset));
  }
  // Inputs: forward and jump, forward, sprint, jump, sneak, a pause.
  const inputs = [17, 1, 65, 16, 0, 33, 1, 17, 0, 1, 0, 49];
  const cases = [];
  for (let first = 0; first < cells.length; first += per) {
    const lines = [`case states-${first}`];
    const group = cells.slice(first, first + per);
    const width = Math.ceil(Math.sqrt(group.length));
    lines.push(`fill -2 -4 -2 ${width * 3 + 2} 0 ${width * 6 + 2} minecraft:stone`);
    group.forEach((state, i) => {
      const x = (i % width) * 3;
      const z = Math.floor(i / width) * 6;
      lines.push(`set ${x} 1 ${z} ${state}`, `set ${x} 0 ${z + 3} ${state}`);
    });
    group.forEach((state, i) => {
      const x = (i % width) * 3;
      const z = Math.floor(i / width) * 6;
      for (const at of [z, z + 3]) {
        lines.push(`player ${x + 0.5} 1 ${at + 0.5} 0 0 0`);
        for (const bits of inputs) lines.push(`tick ${bits} 0 0`);
      }
    });
    lines.push('end');
    cases.push(lines.join('\n'));
  }
  return cases;
}

// --- Traces.
function parseTrace(text) {
  const cases = new Map();
  let name = null;
  for (const line of text.split('\n')) {
    if (line.startsWith('case ')) {
      name = line.slice(5);
      cases.set(name, { lines: [], status: 'ok' });
    } else if (line.startsWith('refused ')) {
      const [, id, ...rest] = line.split(' ');
      cases.set(id, { lines: [], status: 'refused', reason: rest.join(' ') });
    } else if (line.startsWith('error ')) {
      const [, id, ...rest] = line.split(' ');
      cases.set(id, { lines: [], status: 'error', reason: rest.join(' ') });
    } else if (line && name) {
      cases.get(name).lines.push(line);
    }
  }
  return cases;
}

function compare(rust, java, show = 3) {
  let matched = 0;
  let refused = 0;
  const reasons = new Map();
  const failures = [];
  for (const [name, expected] of java) {
    const got = rust.get(name);
    if (!got) {
      failures.push({ name, detail: 'the port printed nothing', errored: true });
    } else if (got.status === 'refused') {
      refused += 1;
      const reason = got.reason.replace(/\[[^\]]*\]/g, '[..]');
      reasons.set(reason, (reasons.get(reason) || 0) + 1);
    } else if (got.status === 'error') {
      failures.push({ name, detail: `the port failed: ${got.reason}`, errored: true });
    } else if (expected.status === 'error') {
      failures.push({ name, detail: `the client failed: ${expected.reason}`, errored: true });
    } else {
      const n = Math.max(expected.lines.length, got.lines.length);
      let at = -1;
      for (let i = 0; i < n; i += 1) {
        if (expected.lines[i] !== got.lines[i]) {
          at = i;
          break;
        }
      }
      if (at < 0) matched += 1;
      else failures.push({ name, detail: `tick ${at - 1}\n  client ${expected.lines[at - 1] || ''}\n    port ${got.lines[at - 1] || ''}\n  client ${expected.lines[at]}\n    port ${got.lines[at]}` });
    }
  }
  return { matched, refused, reasons, failures };
}

async function main() {
  const dataDir = process.env.MCBENCH_DATA_DIR;
  if (!dataDir) throw new Error('MCBENCH_DATA_DIR is not set (a scratch copy of data/, never the real one)');
  const ref = await prepare(dataDir);
  const core = path.join(ROOT, 'core');
  try {
    execFileSync(rust().cargo, ['build', '--release', '--quiet'], { cwd: core, env: rust().env, stdio: ['ignore', 'pipe', 'pipe'] });
  } catch (error) {
    throw new Error(`cargo build failed:\n${error.stderr}`);
  }
  const binary = path.join(core, 'target', 'release', 'mcbench-core');
  const keep = option('keep', fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-movement-')));
  fs.mkdirSync(keep, { recursive: true });

  let casesFile = option('cases', null);
  if (!casesFile && option('builds', null) !== null) {
    const rng = mulberry32(Number(option('seed', 1)));
    const placed = await placedBuilds(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-movement-engine-')), new RegExp(option('builds')), Number(option('limit', Infinity)), process.argv.includes('--saved') ? dataDir : null);
    const autojump = process.argv.includes('--autojump');
    const cases = placed.map(([name, blueprint]) => buildCase(rng, name, blueprint, { players: Number(option('players', 6)), ticks: Number(option('ticks', 200)), autojump }));
    // The build's own pistons, moving with a player in their way.
    for (const [name, blueprint] of placed) {
      const pistons = buildPistons(blueprint).sort(() => rng() - 0.5).slice(0, Number(option('pistons', 4)));
      pistons.forEach((piston, i) => cases.push(buildPistonCase(rng, `${name}-piston${i}`, blueprint, piston, { autojump })));
    }
    casesFile = path.join(keep, 'cases.txt');
    fs.writeFileSync(casesFile, `${cases.join('\n')}\n`);
  }
  if (!casesFile && process.argv.includes('--states')) {
    casesFile = path.join(keep, 'cases.txt');
    fs.writeFileSync(casesFile, `${stateCases(loadRegistry(), new RegExp(option('match', '.')), Number(option('per', 60))).join('\n')}\n`);
  }
  if (!casesFile) {
    const registry = loadRegistry();
    const seed = Number(option('seed', 1));
    const count = Number(option('fuzz', 300));
    const ticks = Number(option('ticks', 200));
    const rng = mulberry32(seed);
    const cases = [];
    for (let i = 0; i < count; i += 1) cases.push((rng() < 0.25 ? generatePiston : generate)(rng, registry, `fuzz${seed}-${i}`, { ticks, autojump: process.argv.includes('--autojump') }));
    casesFile = path.join(keep, 'cases.txt');
    fs.writeFileSync(casesFile, `${cases.join('\n')}\n`);
  }
  const started = Date.now();
  const rustText = execFileSync(binary, ['--movement', casesFile], { maxBuffer: 1 << 30, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
  fs.writeFileSync(path.join(keep, 'rust.txt'), rustText);
  const rustMs = Date.now() - started;
  const javaText = ref.run(casesFile);
  fs.writeFileSync(path.join(keep, 'java.txt'), javaText);
  const result = compare(parseTrace(rustText), parseTrace(javaText));
  const total = result.matched + result.refused + result.failures.length;
  console.log(`${total} cases: ${result.matched} matched, ${result.refused} refused, ${result.failures.length} mismatched (port ${rustMs} ms; kept in ${keep})`);
  for (const [reason, n] of [...result.reasons].sort((a, b) => b[1] - a[1]).slice(0, 10)) console.log(`  refused x${n}: ${reason}`);
  for (const failure of result.failures.slice(0, 5)) console.log(`MISMATCH ${failure.name}: ${failure.detail}`);
  // Only the port refuses (MovementReference.java runs every case).
  const errored = result.failures.filter((failure) => failure.errored).length;
  const differed = result.failures.length - errored;
  console.log(`${result.matched} matched, ${differed} differed, 0 refused by both, ${result.refused} refused by the port alone, ${errored} errored`);
  // A run that compared nothing proves nothing.
  if (result.failures.length || !result.matched) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
