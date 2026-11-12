'use strict';

// Live-world cases (scripts/core-parity.js --orbs N) for experience orbs and
// the golems a carved pumpkin makes. The live world takes only blocks, edits,
// ticks and inputs, so an orb is made the way a world makes one: a furnace
// that smelted gives its recipes' experience when TNT breaks it. Each rig is
// a furnace with a hopper over it, cactus on the hopper that breaks (the
// cactus falls into the hopper, the furnace smelts it into green dye, one
// experience each, a whole number, so no Math.random), a hopper at its side
// fed with ladders an edit takes the wall from (the fuel), and TNT a lever
// primes when the smelting is done, or partly done. Around the rigs: water,
// lava, slime, ice, a wooden pressure plate and a lamp, ores and sculk the
// explosions break (no experience without a player), and orbs land on all of
// it, merge and burn. With `golems`, a case instead completes golem patterns
// in an edit (snow, iron, copper with copper chests beside it): the golems are
// made and the patterns cleared on both engines, and the Rust engine refuses
// at their first tick.

const FACINGS = ['north', 'south', 'east', 'west'];
const STEP = { north: [0, -1], south: [0, 1], east: [1, 0], west: [-1, 0] };
const OPPOSITE = { north: 'south', south: 'north', east: 'west', west: 'east' };
const CLOCKWISE = { north: 'east', east: 'south', south: 'west', west: 'north' };

function orbCase(r, seed) {
  const commands = ['fill 0 62 0 47 63 47 minecraft:stone'];
  const edits = new Map();
  const inputs = [];
  const taken = new Set();
  const free = (cells) => cells.every(([x, y, z]) => !taken.has(`${x},${y},${z}`) && x >= 1 && z >= 1 && x <= 46 && z <= 46);
  const take = (cells) => cells.forEach(([x, y, z]) => taken.add(`${x},${y},${z}`));
  const set = (x, y, z, block) => { commands.push(`setblock ${x} ${y} ${z} ${block}`); take([[x, y, z]]); };
  const edit = (tick, command) => { if (!edits.has(tick)) edits.set(tick, []); edits.get(tick).push(command); };
  let last = 0;
  const rigs = 2 + r.int(3);
  for (let n = 0; n < rigs; n++) {
    for (let attempt = 0; attempt < 20; attempt++) {
      const [fx, fz] = [4 + r.int(40), 4 + r.int(40)];
      const facing = r.pick(FACINGS);
      // The fuel hopper's side, the ladders' facing (across it), the TNT's side.
      const side = r.pick(FACINGS);
      const across = r.next() < 0.5 ? CLOCKWISE[side] : OPPOSITE[CLOCKWISE[side]];
      const tntSide = OPPOSITE[side];
      const [sx, sz] = STEP[side];
      const [ax, az] = STEP[across];
      const [tx, tz] = STEP[tntSide];
      const cactus = 1 + r.int(5);
      const ladders = Math.ceil((cactus * 200) / 300) + r.int(2);
      const cells = [[fx, 64, fz], [fx, 65, fz], ...Array.from({ length: cactus }, (_, i) => [fx, 66 + i, fz]),
        [fx + sx, 64, fz + sz], ...Array.from({ length: ladders }, (_, i) => [fx + sx, 65 + i, fz + sz]), ...Array.from({ length: ladders }, (_, i) => [fx + sx - ax, 65 + i, fz + sz - az]),
        [fx + tx, 64, fz + tz], [fx + 2 * tx, 64, fz + 2 * tz]];
      if (!free(cells)) continue;
      const kind = r.next() < 0.85 ? 'furnace' : r.pick(['smoker', 'blast_furnace']);
      set(fx, 64, fz, `minecraft:${kind}[facing=${facing},lit=false]`);
      set(fx, 65, fz, 'minecraft:hopper[enabled=true,facing=down]');
      for (let i = 0; i < cactus; i++) set(fx, 66 + i, fz, 'minecraft:cactus[age=0]');
      set(fx + sx, 64, fz + sz, `minecraft:hopper[enabled=true,facing=${OPPOSITE[side]}]`);
      for (let i = 0; i < ladders; i++) {
        set(fx + sx - ax, 65 + i, fz + sz - az, 'minecraft:stone');
        set(fx + sx, 65 + i, fz + sz, `minecraft:ladder[facing=${across},waterlogged=false]`);
      }
      // The wall goes, the ladders fall into the fuel hopper.
      edit(3 + r.int(30), `fill ${fx + sx - ax} 65 ${fz + sz - az} ${fx + sx - ax} ${64 + ladders} ${fz + sz - az} minecraft:air`);
      set(fx + tx, 64, fz + tz, 'minecraft:tnt[unstable=false]');
      set(fx + 2 * tx, 64, fz + 2 * tz, `minecraft:lever[face=floor,facing=${tntSide},powered=false]`);
      // The smelting takes 200 ticks a cactus once the fuel is in: the lever
      // is pulled when it is done, or part way.
      const when = 60 + Math.floor(cactus * 200 * (r.next() < 0.75 ? 1.05 : r.next())) + r.int(40);
      inputs.push([when, ['toggle', fx + 2 * tx, 64, fz + 2 * tz, 0]]);
      last = Math.max(last, when + 80);
      // What the explosion and the orbs meet nearby.
      // A fluid replaces the floor under a cell nothing stands on.
      const near = (dx, dz, block) => {
        const fluid = block.includes('water') || block.includes('lava');
        const cells = fluid ? [[fx + dx, 63, fz + dz], [fx + dx, 64, fz + dz]] : [[fx + dx, 64, fz + dz]];
        if (!free(cells)) return;
        take(cells);
        set(...cells[0], block);
      };
      for (let k = 0; k < 2 + r.int(4); k++) {
        const [dx, dz] = [r.int(7) - 3, r.int(7) - 3];
        const block = r.pick(['minecraft:coal_ore', 'minecraft:redstone_ore[lit=false]', 'minecraft:diamond_ore', 'minecraft:sculk', 'minecraft:sculk_catalyst[bloom=false]',
          'minecraft:oak_pressure_plate[powered=false]', 'minecraft:light_weighted_pressure_plate[power=0]', 'minecraft:slime_block', 'minecraft:packed_ice', 'minecraft:soul_sand',
          'minecraft:water[level=0]', 'minecraft:lava[level=0]', 'minecraft:cobweb', 'minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]', 'minecraft:redstone_lamp[lit=false]',
          'minecraft:fire[age=0,east=false,north=false,south=false,up=false,west=false]', 'minecraft:honey_block', 'minecraft:magma_block']);
        near(dx, dz, block);
      }
      break;
    }
  }
  // Some cases also a pool for the orbs to fall into, one level down.
  if (r.next() < 0.5) {
    const [px, pz] = [2 + r.int(36), 2 + r.int(36)];
    const cells = [];
    for (let x = px; x <= px + 5; x++) for (let z = pz; z <= pz + 5; z++) cells.push([x, 63, z], [x, 64, z]);
    if (free(cells)) commands.push(`fill ${px} 63 ${pz} ${px + 5} 63 ${pz + 5} minecraft:${r.pick(['water', 'lava'])}[level=0]`);
  }
  const steps = [{ place: commands, fresh: true }];
  const end = last + 100 + r.int(200);
  const events = [...[...edits].map(([tick, place]) => [tick, { place, fresh: false }]), ...inputs.map(([tick, input]) => [tick, { input }])].sort((a, b) => a[0] - b[0]);
  let t = 0;
  for (const [tick, step] of events) {
    if (tick > t) steps.push({ tick: tick - t });
    t = Math.max(t, tick);
    steps.push(step);
  }
  // The rest in requests of up to 50 ticks, so the engines are compared often.
  while (t < end) {
    const n = Math.min(50, end - t);
    steps.push({ tick: n });
    t += n;
  }
  return { name: `orbs-${seed}`, steps };
}

// Golem patterns a pumpkin completes, in the fresh placement (put last) or
// in an edit after it: a snow golem, an iron golem (the pumpkin on top, air
// at the shoulders' sides, or a block there: no golem), a copper golem on
// each copper block, waxed or not, with copper chests beside it that the new
// chest joins (single, left or right, least oxidized of the two), the pumpkin
// facing any way.
function golemCase(r, seed) {
  const commands = ['fill 0 62 0 31 63 31 minecraft:stone'];
  const edit = [];
  const copper = ['copper_block', 'exposed_copper', 'weathered_copper', 'oxidized_copper', 'waxed_copper_block', 'waxed_exposed_copper', 'waxed_weathered_copper', 'waxed_oxidized_copper'];
  const chests = ['copper_chest', 'exposed_copper_chest', 'weathered_copper_chest', 'oxidized_copper_chest', 'waxed_copper_chest', 'waxed_exposed_copper_chest', 'waxed_weathered_copper_chest', 'waxed_oxidized_copper_chest', 'chest'];
  const pumpkin = () => `minecraft:${r.pick(['carved_pumpkin', 'jack_o_lantern'])}[facing=${r.pick(FACINGS)}]`;
  for (let n = 0; n < 1 + r.int(4); n++) {
    const [x, z] = [2 + 6 * (n % 4), 2 + 6 * Math.floor(n / 4) + r.int(3)];
    const kind = r.pick(['snow', 'iron', 'copper', 'copper', 'copper']);
    if (kind === 'snow') {
      commands.push(`setblock ${x} 64 ${z} minecraft:snow_block`, `setblock ${x} 65 ${z} minecraft:snow_block`);
      edit.push(`setblock ${x} 66 ${z} ${pumpkin()}`);
    } else if (kind === 'iron') {
      const alongX = r.next() < 0.5;
      const [dx, dz] = alongX ? [1, 0] : [0, 1];
      commands.push(`setblock ${x} 64 ${z} minecraft:iron_block`, `setblock ${x - dx} 65 ${z - dz} minecraft:iron_block`, `setblock ${x} 65 ${z} minecraft:iron_block`, `setblock ${x + dx} 65 ${z + dz} minecraft:iron_block`);
      if (r.next() < 0.2) commands.push(`setblock ${x + dx} 66 ${z + dz} minecraft:stone`);
      edit.push(`setblock ${x} 66 ${z} ${pumpkin()}`);
    } else {
      commands.push(`setblock ${x} 64 ${z} minecraft:${r.pick(copper)}`);
      for (const facing of FACINGS) {
        if (r.next() < 0.4) {
          const [dx, dz] = STEP[facing];
          commands.push(`setblock ${x + dx} 64 ${z + dz} minecraft:${r.pick(chests)}[facing=${r.pick(FACINGS)},type=${r.pick(['single', 'single', 'left', 'right'])},waterlogged=false]`);
        }
      }
      edit.push(`setblock ${x} 65 ${z} ${pumpkin()}`);
    }
  }
  const steps = r.next() < 0.5 ? [{ place: [...commands, ...edit], fresh: true }] : [{ place: commands, fresh: true }, { tick: 2 }, { place: edit, fresh: false }];
  return { name: `golems-${seed}`, steps: [...steps, { tick: 1 }] };
}

function orbCases(count, seed, random) {
  return Array.from({ length: count }, (_, i) => {
    const r = random(seed + i);
    return i % 4 === 3 ? golemCase(r, seed + i) : orbCase(r, seed + i);
  });
}

// Experience orbs a verifier experiment of the fuzzer imports
// (scripts/core-verify-parity.js --fuzz), from a random stream of their own
// (seeded by the world, so the experiment's other draws are the same with or
// without them): orbs above the blocks, some in clusters of one value large
// enough to merge, with any saved fields; and recipes used by the furnaces
// that have no saved data, in whole experience (so that TNT breaking them
// gives orbs).
function addOrbImports(placed, args, random) {
  let hash = 2166136261;
  for (const block of placed) for (const c of `${block.id}${block.pos.join(',')};`) hash = Math.imul(hash ^ c.charCodeAt(0), 16777619);
  const r = random(hash >>> 0);
  if (r.next() < 0.4 || !placed.length) return;
  const { orbTag } = require('./orb-fixtures');
  const entities = args.entities || [];
  let n = 90000;
  const fields = () => {
    const list = [`Value:${r.pick([0, 1, 3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477, 5, 2, 100000])}`];
    if (r.next() < 0.3) list.push(`Count:${1 + r.int(5)}`);
    if (r.next() < 0.2) list.push(`Age:${r.pick([0, 100, 5990, 5999, -5])}s`);
    if (r.next() < 0.2) list.push(`Health:${r.pick([1, 2, 5, 30])}s`);
    if (r.next() < 0.15) list.push(`Fire:${r.int(100)}s`);
    if (r.next() < 0.1) list.push('NoGravity:1b');
    if (r.next() < 0.1) list.push('Invulnerable:1b');
    if (r.next() < 0.1) list.push('"Paper.ExpData":{reason:"BLOCK_BREAK"}');
    if (r.next() < 0.3) list.push(`Motion:[${(r.int(9) - 4) / 10}d,${(r.int(5) - 2) / 10}d,${(r.int(9) - 4) / 10}d]`);
    return list.join(',');
  };
  for (let k = 1 + r.int(6); k > 0; k--) {
    const base = r.pick(placed).pos;
    const at = [base[0] + 0.5, base[1] + 1 + r.int(3) + r.int(4) / 4, base[2] + 0.5];
    if (r.next() < 0.25) {
      // A cluster of one value: ids 40 apart merge.
      const value = r.pick([1, 3, 7]);
      for (let i = 0, count = 41 + r.int(10); i < count; i++) entities.push(orbTag((n += 10), at, `Value:${value}`));
    } else {
      entities.push(orbTag((n += 10), at, fields()));
    }
  }
  // Some orbs saved by an older version (UUIDs as int arrays from data version 2514), which the data fixer upgrades.
  if (r.next() < 0.3) {
    const version = r.pick([2586, 2730, 2975, 3465, 3700, 4189]);
    for (const entity of entities) if (/experience_orb/.test(entity.snbt) && r.next() < 0.5) entity.data_version = version;
  }
  args.entities = entities;
  const recipes = [['minecraft:green_dye', 1 + r.int(9)], ['minecraft:iron_ingot_from_smelting_raw_iron', 10], ['minecraft:cooked_beef_from_smoking', 20], ['minecraft:diamond_from_blasting_diamond_ore', 2 + r.int(5)]];
  const saved = new Set((args.block_entities || []).map((entry) => entry.at.join(',')));
  for (const block of placed) {
    const short = block.id.replace('minecraft:', '');
    if (!/^(furnace|blast_furnace|smoker)$/.test(short) || saved.has(block.pos.join(',')) || r.next() < 0.3) continue;
    const [id, count] = r.pick(recipes);
    args.block_entities = [...(args.block_entities || []), { at: block.pos, data_version: 4556, snbt: `{id:"minecraft:${short}",RecipesUsed:{"${id}":${count}}}` }];
  }
}

module.exports = { orbCases, addOrbImports };
