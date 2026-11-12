'use strict';

// Piston labs for scripts/core-parity.js --case: small worlds with a piston, cargo (slime, honey,
// slabs, stairs, fences...), items dropped by breaking blocks, falling blocks and TNT, in which
// moving blocks push entities.
//   node scripts/piston-labs.js <folder> <count> <first seed>   prints the files, comma-separated

const fs = require('fs');
const path = require('path');
const out = process.argv[2];
const count = Number(process.argv[3] || 20);
const base = Number(process.argv[4] || 1);

function random(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return { next, int: (n) => Math.floor(next() * n), pick: (a) => a[Math.floor(next() * a.length)] };
}
const DIRS = { north: [0, 0, -1], south: [0, 0, 1], west: [-1, 0, 0], east: [1, 0, 0], up: [0, 1, 0], down: [0, -1, 0] };
const HORIZ = ['north', 'south', 'west', 'east'];
const CARGO = ['minecraft:stone', 'minecraft:slime_block', 'minecraft:slime_block', 'minecraft:slime_block', 'minecraft:honey_block', 'minecraft:honey_block', 'minecraft:honey_block', 'minecraft:glass', 'minecraft:oak_planks',
  'minecraft:oak_slab[type=bottom]', 'minecraft:oak_slab[type=top]', 'minecraft:oak_stairs[facing=north,half=bottom,shape=straight]', 'minecraft:oak_stairs[facing=east,half=top,shape=outer_left]',
  'minecraft:oak_fence', 'minecraft:cobblestone_wall', 'minecraft:oak_trapdoor[half=bottom,open=false]', 'minecraft:iron_bars', 'minecraft:snow[layers=3]', 'minecraft:hay_block', 'minecraft:soul_sand',
  'minecraft:target', 'minecraft:daylight_detector', 'minecraft:redstone_lamp', 'minecraft:farmland[moisture=0]', 'minecraft:dirt_path', 'minecraft:oak_leaves', 'minecraft:oak_pressure_plate', 'minecraft:rail'];
const FRAGILE = ['minecraft:torch', 'minecraft:redstone_wire', 'minecraft:rail', 'minecraft:stone_button[face=floor,facing=north]', 'minecraft:oak_pressure_plate', 'minecraft:lever[face=floor,facing=north]',
  'minecraft:redstone_torch', 'minecraft:repeater[facing=north]', 'minecraft:comparator[facing=north]', 'minecraft:white_carpet', 'minecraft:short_grass', 'minecraft:oak_sapling'];
const FALL = ['minecraft:sand', 'minecraft:gravel', 'minecraft:anvil[facing=north]'];

function lab(seed) {
  const r = random(seed);
  const size = 12;
  const commands = [`fill 0 0 0 ${size - 1} 0 ${size - 1} minecraft:stone`];
  const steps = [];
  const timeline = new Map();
  const at = (t, cmd) => { if (!timeline.has(t)) timeline.set(t, []); timeline.get(t).push(cmd); };
  const type = r.int(7);
  const sticky = r.next() < 0.5;
  const pkey = sticky ? 'minecraft:sticky_piston' : 'minecraft:piston';
  const x = 4 + r.int(4), z = 4 + r.int(4);
  let facing, y, chain = 1 + r.int(3), ptime, utime;
  const cargo = Array.from({ length: 4 }, () => r.pick(CARGO));
  ptime = 22 + r.int(10);
  utime = ptime + 8 + r.int(14);
  const dropTime = () => 1 + r.int(4);
  const droppers = [];
  const addDropper = (dx, dy, dz, fragile) => {
    const t = dropTime();
    commands.push(`setblock ${dx} ${dy} ${dz} minecraft:glass`);
    commands.push(`setblock ${dx} ${dy + 1} ${dz} ${fragile || r.pick(FRAGILE)}`);
    at(t, `setblock ${dx} ${dy} ${dz} minecraft:air`);
  };
  const power = (cx, cy, cz) => { at(ptime, `setblock ${cx} ${cy} ${cz} minecraft:redstone_block`); if (r.next() < 0.85) at(utime, `setblock ${cx} ${cy} ${cz} minecraft:air`); };
  if (type === 0) {
    // Lift: a piston facing up, cargo above, items dropped onto the cargo.
    facing = 'up'; y = 1;
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=false,facing=up]`);
    for (let k = 0; k < chain; k++) commands.push(`setblock ${x} ${2 + k} ${z} ${cargo[k]}`);
    const top = 1 + chain;
    for (let i = 0, n = 2 + r.int(4); i < n; i++) addDropper(x + (r.next() < 0.7 ? 0 : r.pick([-1, 1])), top + 2 + r.int(2), z + (r.next() < 0.7 ? 0 : r.pick([-1, 1])));
    power(x + 1, 1, z);
  } else if (type === 1) {
    // Side: a piston facing sideways at the floor, cargo in front, items on the floor ahead and on the cargo.
    facing = r.pick(HORIZ); y = 1;
    const [dx, , dz] = DIRS[facing];
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=false,facing=${facing}]`);
    for (let k = 0; k < chain; k++) commands.push(`setblock ${x + dx * (1 + k)} 1 ${z + dz * (1 + k)} ${cargo[k]}`);
    for (let i = 0, n = 2 + r.int(5); i < n; i++) {
      const d = 1 + r.int(chain + 2);
      const lx = x + dx * d + (dx === 0 ? r.pick([-1, 0, 0, 1]) : 0), lz = z + dz * d + (dz === 0 ? r.pick([-1, 0, 0, 1]) : 0);
      addDropper(lx, 2 + r.int(2), lz);
    }
    const side = dx === 0 ? [x + 1, 1, z] : [x, 1, z + 1];
    power(...side);
  } else if (type === 2) {
    // Down: items rest on the floor, cargo is placed above them, the piston pushes it down.
    facing = 'down'; y = 4;
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=false,facing=down]`);
    for (let i = 0, n = 2 + r.int(4); i < n; i++) addDropper(x + (r.next() < 0.7 ? 0 : r.pick([-1, 1])), 3 + r.int(2), z + (r.next() < 0.7 ? 0 : r.pick([-1, 1])));
    const cargoTime = 22;
    ptime = cargoTime + 2 + r.int(4);
    utime = ptime + 10 + r.int(10);
    at(cargoTime, `setblock ${x} 3 ${z} ${cargo[0]}`);
    if (chain > 1) at(cargoTime, `setblock ${x} 2 ${z} ${cargo[1]}`);
    power(x + 1, y, z);
  } else if (type === 3) {
    // Head: an extended piston facing up, items on and above its head; it retracts and extends.
    facing = 'up'; y = 1;
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=true,facing=up]`);
    commands.push(`setblock ${x} 2 ${z} minecraft:piston_head[facing=up,short=false,type=${sticky ? 'sticky' : 'normal'}]`);
    if (r.next() < 0.6) commands.push(`setblock ${x} 3 ${z} ${cargo[0]}`);
    for (let i = 0, n = 2 + r.int(4); i < n; i++) addDropper(x + (r.next() < 0.6 ? 0 : r.pick([-1, 1])), 4 + r.int(2), z + (r.next() < 0.6 ? 0 : r.pick([-1, 1])));
    commands.push(`setblock ${x + 1} 1 ${z} minecraft:redstone_block`);
    at(ptime, `setblock ${x + 1} 1 ${z} minecraft:air`);
    if (r.next() < 0.8) at(utime, `setblock ${x + 1} 1 ${z} minecraft:redstone_block`);
  } else if (type === 5) {
    // TNT: primed beside a piston with cargo, pushed about.
    facing = r.pick(HORIZ); y = 1;
    const [dx, , dz] = DIRS[facing];
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=false,facing=${facing}]`);
    for (let k = 0; k < chain; k++) commands.push(`setblock ${x + dx * (1 + k)} 1 ${z + dz * (1 + k)} ${cargo[k]}`);
    const lx = x + dx * (chain + 1), lz = z + dz * (chain + 1);
    commands.push(`setblock ${lx} 1 ${lz} minecraft:tnt`);
    commands.push(`setblock ${lx} 2 ${lz} minecraft:tnt`);
    at(ptime - 6, `setblock ${lx + (dz !== 0 ? 1 : 0)} 1 ${lz + (dx !== 0 ? 1 : 0)} minecraft:redstone_block`);
    const side = dx === 0 ? [x + 1, 1, z] : [x, 1, z + 1];
    power(...side);
    ptime = Math.min(ptime, 30);
  } else if (type === 6) {
    // Honey and slime side pushes with items on top of the cargo.
    facing = r.pick(HORIZ); y = 1;
    const [dx, , dz] = DIRS[facing];
    commands.push(`setblock ${x} ${y} ${z} ${pkey}[extended=false,facing=${facing}]`);
    const kind = r.pick(['minecraft:honey_block', 'minecraft:slime_block', 'minecraft:honey_block']);
    for (let k = 0; k < chain; k++) {
      commands.push(`setblock ${x + dx * (1 + k)} 1 ${z + dz * (1 + k)} ${r.next() < 0.7 ? kind : cargo[k]}`);
      if (r.next() < 0.8) addDropper(x + dx * (1 + k), 3 + r.int(2), z + dz * (1 + k));
    }
    for (let i = 0, n = 1 + r.int(3); i < n; i++) addDropper(x + dx * (1 + r.int(chain + 2)) + (dx === 0 ? r.pick([-1, 0, 1]) : 0), 3, z + dz * (1 + r.int(chain + 2)) + (dz === 0 ? r.pick([-1, 0, 1]) : 0));
    const side = dx === 0 ? [x + 1, 1, z] : [x, 1, z + 1];
    power(...side);
  } else {
    // Sticky side pull: an extended sticky piston facing sideways with cargo on its head, items beside.
    facing = r.pick(HORIZ); y = 1;
    const [dx, , dz] = DIRS[facing];
    commands.push(`setblock ${x} ${y} ${z} minecraft:sticky_piston[extended=true,facing=${facing}]`);
    commands.push(`setblock ${x + dx} ${y} ${z + dz} minecraft:piston_head[facing=${facing},short=false,type=sticky]`);
    for (let k = 0; k < chain; k++) commands.push(`setblock ${x + dx * (2 + k)} 1 ${z + dz * (2 + k)} ${cargo[k]}`);
    for (let i = 0, n = 2 + r.int(5); i < n; i++) {
      const d = r.int(chain + 3);
      const lx = x + dx * d + (dx === 0 ? r.pick([-1, 0, 0, 1]) : 0), lz = z + dz * d + (dz === 0 ? r.pick([-1, 0, 0, 1]) : 0);
      addDropper(lx, 2 + r.int(2), lz);
    }
    const side = dx === 0 ? [x + 1, 1, z] : [x, 1, z + 1];
    commands.push(`setblock ${side.join(' ')} minecraft:redstone_block`);
    at(ptime, `setblock ${side.join(' ')} minecraft:air`);
    if (r.next() < 0.8) at(utime, `setblock ${side.join(' ')} minecraft:redstone_block`);
  }
  if (r.next() < 0.3) {
    const fx = x - 1 + r.int(3), fz = z - 1 + r.int(3);
    commands.push(`setblock ${fx} 8 ${fz} minecraft:glass`);
    commands.push(`setblock ${fx} 9 ${fz} ${r.pick(FALL)}`);
    at(1 + r.int(12), `setblock ${fx} 8 ${fz} minecraft:air`);
  }
  steps.push({ place: commands, fresh: true });
  const last = Math.max(...timeline.keys()) + 14 + r.int(12);
  for (let t = 0; t <= last; t++) {
    if (timeline.has(t)) steps.push({ place: timeline.get(t), fresh: false });
    steps.push({ tick: 1 });
  }
  return steps;
}

fs.mkdirSync(out, { recursive: true });
const files = [];
for (let i = 0; i < count; i++) {
  const seed = base + i;
  const file = path.join(out, `typed-${seed}.json`);
  fs.writeFileSync(file, JSON.stringify(lab(seed)));
  files.push(file);
}
console.log(files.join(','));
