'use strict';

// Checks the Rust engine's VERIFY (core/src/verify) against the Java engine's
// (McbenchEngine.verify, running McbenchExperiment on the pinned server's own
// classes) byte for byte. Each case is one verifier request, with the
// commands that place its world, that both engines run into a folder of their
// own; report.json and result.json must be identical but for what differs by
// nature (the engine's name, the timings in `metrics` and the byte count of
// a report that holds those timings): a seeded result holds the saved chunks
// (blocks, light, heightmaps, block entities, ticks) of its columns, which
// must be identical too. A request the Rust engine refuses ("unported: ...")
// counts as refused, not as a match.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-parity.js --verify
//     [--fixtures]                        scripts/verifier-fixtures.js
//     [--fuzz N] [--seed S] [--families a,b] [--drops] [--shift]   random experiments
//     [--requests folder]                 recorded experiments (a folder each of request.json and commands.txt), seeded with S
//     [--placement updates|strict|both]   (community builds; default both)
//     [--match regex] [--limit N] [--ticks N] [--every N]
//     [--keep folder] [--save folder] [--native] [--timing] [--no-inserts] [--unseeded]
//     [--base binary]                     an earlier Rust build as the first engine
//     [--rust binary]                     a Rust build made elsewhere (an earlier commit's) as the Rust engine
//
// Without --fixtures or --fuzz the cases are the community builds in
// the corpus MCBENCH_CORPUS names, each with the experiment scripts/ticker-parity.js
// makes of it (every redstone component probed, levers toggled, buttons
// pressed, containers probed and filled, items dropped, a container drained).
// --no-inserts leaves out the community experiments' inserts and drains (the
// Rust engine refuses them until its containers hold items).
// Requests are seeded, so both engines draw the same random numbers and
// number their entities the same way.
//
// --unseeded sends no seed, as the app does: the engines then number their
// entities and make their UUIDs differently (Java's come from state the engine
// process carries from one experiment to the next), so only the experiments
// that make no entity are compared.
//
// --timing runs every case a second time without a seed, on both engines, for
// the time each takes (the Java engine on its JVM is warm by then).

const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { execFileSync, spawn } = require('child_process');
const ROOT = path.join(__dirname, '..');
const { minecraftLive } = require('../server/minecraft-live');
const { Engine, engineEnv, jvmEngine, coreBuildArgs, rust } = require('../server/engine');
const { MinecraftVerifier } = require('../server/minecraft-verify');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { BUDGETS } = require('../server/verification-contract');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};
const PLACEMENT_MODES = ['updates', 'strict'];

const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;
const CONTAINERS = /chest$|barrel|hopper$|dropper|dispenser|furnace$|smoker|shulker_box/;

// --- Cases: { name, placementMode, load() -> { world, args, config } } ---

function communityBuilds() {
  const builds = new Map(corpusArtifacts());
  return [...builds].sort(([a], [b]) => a.localeCompare(b));
}

// scripts/ticker-parity.js's experiment of a community build: every redstone
// component probed, each lever toggled twice and each button pressed; up to
// 16 containers probed with their inventories, items put into up to 8 of
// them and the first drained; the world's inventories observed.
function experiment(artifact, ticks, every) {
  const probes = [];
  const actions = [];
  const containers = artifact.blocks.filter((block) => CONTAINERS.test(block.id)).slice(0, 16).map((block) => block.pos);
  probes.push(...containers);
  for (const block of artifact.blocks) {
    if (!COMPONENTS.test(block.id)) continue;
    if (probes.length < 256 && !containers.includes(block.pos)) probes.push(block.pos);
    if (block.id === 'minecraft:lever') actions.push({ tick: 2, type: 'toggle', at: block.pos }, { tick: Math.floor(ticks / 2), type: 'toggle', at: block.pos });
    else if (block.id.endsWith('_button')) actions.push({ tick: 4, type: 'press', at: block.pos });
  }
  containers.slice(0, 8).forEach((at, index) => actions.push({ tick: 3 + index, type: 'insert', at, slot: 0, count: 16, item: 'minecraft:cobblestone' }));
  const hopper = artifact.blocks.find((block) => block.id === 'minecraft:hopper');
  if (hopper) actions.push({ tick: 6, type: 'drop', at: [hopper.pos[0] + 0.5, hopper.pos[1] + 1.5, hopper.pos[2] + 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] });
  if (containers.length) actions.push({ tick: ticks - 10, type: 'drain', at: containers[0] });
  if (!probes.length) probes.push(artifact.blocks[0].pos);
  return {
    probes, actions, samples: Array.from({ length: Math.floor(ticks / every) + 1 }, (_, index) => index * every),
    ...(containers.length ? { inventory_probes: containers, inventory_missing: 'report', world_inventories: true } : {}),
    item_entities: true, living_entities: true,
  };
}

function communityCases(ticks, every, placementModes) {
  return communityBuilds().flatMap(([name, file]) => placementModes.map((placementMode) => ({
    name: `${name}/${placementMode}`, placementMode,
    load() {
      const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
      const world = new World();
      for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
      world.finalize();
      const args = experiment(artifact, ticks, every);
      if (process.argv.includes('--no-inserts')) args.actions = args.actions.filter((action) => !['insert', 'extract', 'drain'].includes(action.type));
      return { world, args, config: {} };
    },
  })));
}

function fixtureCases(placementModes) {
  return require('./verifier-fixtures').map((fixture) => ({
    name: fixture.name,
    placementMode: fixture.placementMode ?? placementModes[0],
    load() {
      const world = fixture.world();
      if (!(world instanceof World)) throw new Error(`Fixture ${fixture.name} world() did not return a World`);
      return { world, args: fixture.args, config: {} };
    },
  }));
}

// A random experiment on a random world of the fuzzer's (scripts/core-parity.js):
// probes (blocks of the world, air, a container or not), inputs (levers and
// buttons, right clicks on the blocks that take one, mostly with a yaw, and
// inputs aimed at blocks that cannot take them), dropped items,
// sample ticks (a sequence or a scatter), the censuses, and the report's
// encoding.
function fuzzCases(count, seed, families, drops) {
  const { FAMILIES, USED, yawOf, fuzzCase, registry, random } = require('./core-parity');
  const blocks = registry();
  const names = (families || Object.keys(FAMILIES).join(',')).split(',');
  return Array.from({ length: count }, (_, i) => ({
    name: `fuzz-${seed + i}`,
    placementMode: PLACEMENT_MODES[(seed + i) % 2],
    load() {
      const placementMode = PLACEMENT_MODES[(seed + i) % 2];
      const r = random(seed + i);
      const built = fuzzCase(blocks, names, seed + i, 1, drops, false, process.argv.includes('--shift'));
      const world = new World();
      const placed = [];
      for (const command of built.steps[0].place) {
        const [, x, y, z, text] = /^setblock (-?\d+) (-?\d+) (-?\d+) (\S+)$/.exec(command);
        const bracket = text.indexOf('[');
        const states = {};
        if (bracket >= 0) for (const pair of text.slice(bracket + 1, -1).split(',')) states[pair.split('=')[0]] = pair.split('=')[1];
        const id = bracket < 0 ? text : text.slice(0, bracket);
        world.set(Number(x), Number(y), Number(z), id, states);
        placed.push({ id, pos: [Number(x), Number(y), Number(z)] });
      }
      if (!placed.length) { world.set(0, 0, 0, 'minecraft:stone'); placed.push({ id: 'minecraft:stone', pos: [0, 0, 0] }); }
      world.finalize();
      const ticks = 4 + r.int(36);
      let samples;
      if (r.next() < 0.5) samples = Array.from({ length: Math.floor(ticks / (1 + r.int(3))) + 1 }, (_, k) => k);
      else samples = [...new Set(Array.from({ length: 1 + r.int(8) }, () => r.int(ticks + 1)).concat([ticks]))].sort((a, b) => a - b);
      if (samples.length > 1 && r.next() < 0.5) { const step = 1 + r.int(3); samples = Array.from({ length: Math.floor(ticks / step) + 1 }, (_, k) => k * step); }
      const last = samples.at(-1);
      const probes = [];
      const seen = new Set();
      const probe = (pos) => { const key = pos.join(','); if (!seen.has(key)) { seen.add(key); probes.push(pos); } };
      for (const block of placed) if (block.id !== 'minecraft:stone' || r.next() < 0.1) probe(block.pos);
      for (let n = r.int(6); n > 0; n--) probe([r.int(8) - 1, r.int(4), r.int(8) - 1]);
      if (!probes.length) probe(placed[0].pos);
      probes.length = Math.min(probes.length, 1 + r.int(120));
      seen.clear();
      for (const pos of probes) seen.add(pos.join(','));
      const actions = [];
      const when = () => r.int(last + 1);
      for (const block of placed) {
        if (block.id === 'minecraft:lever' && r.next() < 0.8) for (let n = 1 + r.int(3); n > 0; n--) actions.push({ tick: when(), type: 'toggle', at: block.pos });
        else if (block.id.endsWith('_button') && r.next() < 0.8) for (let n = 1 + r.int(2); n > 0; n--) actions.push({ tick: when(), type: 'press', at: block.pos });
        else if (block.id === 'minecraft:lectern' && r.next() < 0.7) {
          // Books taken and put back (supplied ones added with the imports) and pages turned.
          for (let n = 1 + r.int(4); n > 0; n--) {
            const type = r.next() < 0.3 ? 'page' : 'book';
            actions.push({ tick: when(), type, at: block.pos, ...(type === 'page' ? { page: r.int(4) } : { present: r.next() < 0.5 }) });
          }
        } else if (USED.has(block.id) && r.next() < 0.6) for (let n = 1 + r.int(3); n > 0; n--) actions.push({ tick: when(), type: 'use', at: block.pos, ...(r.next() < 0.85 ? { yaw: yawOf(r) } : {}) });
        else if (r.next() < 0.03) {
          const type = r.pick(['toggle', 'press', 'use', 'page', 'book']);
          actions.push({ tick: when(), type, at: block.pos, ...(type === 'page' ? { page: r.int(3) } : type === 'book' ? { present: r.next() < 0.5 } : type === 'use' && r.next() < 0.5 ? { yaw: yawOf(r) } : {}) });
        }
        // The upper half of a door takes a use as the lower half does.
        if (/_door$/.test(block.id) && r.next() < 0.3) actions.push({ tick: when(), type: 'use', at: [block.pos[0], block.pos[1] + 1, block.pos[2]], yaw: yawOf(r) });
      }
      for (let n = r.int(3); n > 0; n--) {
        const at = r.pick(placed).pos;
        actions.push({ tick: when(), type: 'drop', at: [at[0] + r.next(), at[1] + 1 + r.next(), at[2] + r.next()], item: r.pick(['minecraft:redstone', 'minecraft:cobblestone', 'minecraft:ender_pearl', 'minecraft:stone', 'minecraft:air', 'minecraft:iron_ingot']), count: 1 + r.int(r.next() < 0.1 ? 64 : 8), velocity: r.next() < 0.5 ? [0, 0, 0] : [r.next() - 0.5, r.next() * 0.3, r.next() - 0.5] });
      }
      const args = {};
      const containers = placed.filter((block) => CONTAINERS.test(block.id) && seen.has(block.pos.join(',')));
      if (containers.length && r.next() < 0.7) {
        args.inventory_probes = containers.map((block) => block.pos);
        if (r.next() < 0.5) {
          const other = probes.find((pos) => !containers.some((block) => block.pos.join(',') === pos.join(',')));
          if (other && r.next() < 0.5) args.inventory_probes.push(other);
        }
        args.inventory_missing = r.pick(['report', 'report', 'error']);
        if (r.next() < 0.04) actions.push({ tick: when(), type: 'insert', at: containers[0].pos, slot: 0, count: 3, item: 'minecraft:cobblestone' });
        if (r.next() < 0.04) actions.push({ tick: when(), type: 'extract', at: containers[0].pos, slot: 0, count: 1 });
        if (r.next() < 0.04) actions.push({ tick: when(), type: 'drain', at: containers[0].pos });
      }
      if (r.next() < 0.5) args.item_entities = true;
      if (r.next() < 0.5) args.world_inventories = true;
      if (r.next() < 0.5) args.living_entities = true;
      // A saved tick queue (strict placement only): ticks of blocks in the
      // world, some of other blocks, with delays from the past to the future.
      if (placementMode === 'strict' && r.next() < 0.5) {
        const xs = placed.map((block) => block.pos);
        const lo = [0, 1, 2].map((axis) => Math.min(...xs.map((pos) => pos[axis])) - 1);
        const hi = [0, 1, 2].map((axis) => Math.max(...xs.map((pos) => pos[axis])) + 1);
        const entries = [];
        const taken = new Set();
        for (let n = r.int(12); n > 0; n--) {
          const block = r.pick(placed);
          const id = r.next() < 0.8 ? block.id : r.pick(['minecraft:stone', 'minecraft:redstone_torch', 'minecraft:repeater', 'minecraft:redstone_lamp']);
          const key = `${id}:${block.pos.join(',')}`;
          if (taken.has(key)) continue;
          taken.add(key);
          entries.push({ kind: 'block', id, at: block.pos, delay: r.int(14) - 3, priority: r.int(7) - 3, sub_tick: String(r.pick([-5n, 0n, 3n, 40n, 1000n, 9007199254740993n]) + BigInt(n) * 100000n) });
        }
        args.scheduled_ticks = { bounds: [lo, hi], data_version: r.pick([4556, 4189, 4556]), entries };
      }
      // Saved data: block entities for the blocks that have one, item entities
      // above the blocks, stacks that the inputs supply, and saved fluid ticks.
      if (!process.argv.includes('--no-imports')) {
        addImports(r, placed, args, actions, last);
        require('./orb-cases').addOrbImports(placed, args, random);
      }
      const config = { reportEncoding: r.pick(['full', 'rle-v1']), sprint: r.next() < 0.7 };
      // Some with a report budget the experiment may not fit in (the Rust engine refuses those).
      if (r.next() < 0.05) config.budgets = { ...BUDGETS, reportBytes: 2000 + r.int(80000) };
      if (r.next() < 0.2) args.block_data_version = r.pick([4556, 4556, 4556, 4189, 3465, 2975, 1952, 99999]);
      return { world, args: { ...args, probes, actions, samples, settle_ticks: 1 + r.int(8) }, config };
    },
  }));
}

// The saved data a request may import (see scripts/saved-data-fuzz.js), in
// the server's own data version most of the time.
// Vehicles, paintings and armor stands where the build has rails, hoppers, TNT, water and walls: minecarts of every class on rails (powered,
// detector and activator rails among them), hopper minecarts under dropped items and over hoppers, boats by water, paintings on walls,
// armor stands near TNT. Saved data Minecraft accepts and this engine reads.
function addVehicles(r, placed, args, actions, version) {
  const { cartTag, boatTag, paintingTag, standTag } = require('./verifier-fixtures');
  const entities = args.entities || [];
  let n = 7000 + 10 * entities.length;
  const add = (tag) => { const data_version = version(); if (!process.env.VEHICLE_SKIP || !new RegExp(process.env.VEHICLE_SKIP).test(tag.snbt)) entities.push({ at: tag.at, data_version, snbt: tag.snbt }); };
  const motion = () => `Motion:[${(r.int(7) - 3) / 10}d,0.0d,${(r.int(7) - 3) / 10}d]`;
  const solidItem = r.pick(['cobblestone', 'stone', 'iron_ingot', 'stick']);
  const carts = ['minecart', 'chest_minecart', 'hopper_minecart', 'furnace_minecart', 'tnt_minecart'];
  const cartExtra = (id) => ({
    minecart: () => motion(),
    chest_minecart: () => `${motion()},Items:[{Slot:${r.int(27)}b,id:"minecraft:${solidItem}",count:${1 + r.int(30)}}]`,
    hopper_minecart: () => `${motion()},Items:[{Slot:${r.int(5)}b,id:"minecraft:${solidItem}",count:${1 + r.int(30)}}]${r.chance(0.2) ? ',Enabled:0b' : ''}`,
    furnace_minecart: () => `${motion()},Fuel:${r.int(400)}s`,
    tnt_minecart: () => `${motion()}${r.chance(0.4) ? `,fuse:${5 + r.int(40)}` : ''}`,
  })[id]();
  const rails = placed.filter((b) => /rail$/.test(b.id));
  for (let k = r.int(4); k > 0 && rails.length; k--) {
    const b = r.pick(rails).pos;
    const id = r.pick(carts);
    add(cartTag(n += 10, [b[0] + 0.5, b[1] + 0.0625, b[2] + 0.5], id, cartExtra(id)));
  }
  const hoppers = placed.filter((b) => /hopper$/.test(b.id));
  for (let k = r.int(3); k > 0 && hoppers.length; k--) {
    const b = r.pick(hoppers).pos;
    add(cartTag(n += 10, [b[0] + 0.5, b[1] + 1.0625, b[2] + 0.5], 'hopper_minecart', cartExtra('hopper_minecart')));
  }
  for (const action of actions) {
    if (action.type === 'drop' && action.at && r.chance(0.4)) add(cartTag(n += 10, [action.at[0] + 0.5, action.at[1] - 1 + 0.0625 + 0.9, action.at[2] + 0.5], 'hopper_minecart', cartExtra('hopper_minecart')));
  }
  const water = placed.filter((b) => /water/.test(b.id));
  for (let k = r.int(3); k > 0; k--) {
    const b = r.pick(water.length ? water : placed).pos;
    add(boatTag(n += 10, [b[0] + 0.5, b[1] + 0.3, b[2] + 0.5], r.pick(['oak_boat', 'bamboo_raft', 'oak_chest_boat', 'birch_boat']), motion()));
  }
  const walls = placed.filter((b) => !/wire|rail|lever|button|torch|plate|water|lava|air/.test(b.id));
  for (let k = r.int(4); k > 0 && walls.length; k--) {
    const b = r.pick(walls).pos;
    const facing = r.pick(['north', 'south', 'west', 'east']);
    const delta = { north: [0, -1], south: [0, 1], west: [-1, 0], east: [1, 0] }[facing];
    add(paintingTag(n += 10, [b[0] + delta[0], b[1], b[2] + delta[1]], facing, r.pick(['kebab', 'sea', 'pool', 'graham', 'wither'])));
  }
  const tnt = placed.filter((b) => /tnt/.test(b.id));
  for (let k = r.int(5); k > 0; k--) {
    const b = r.pick(tnt.length ? tnt : placed).pos;
    const extra = [];
    if (r.chance(0.3)) extra.push('Small:1b');
    if (r.chance(0.2)) extra.push('Invisible:1b');
    if (r.chance(0.1)) extra.push('Marker:1b');
    if (r.chance(0.1)) extra.push('NoGravity:1b');
    if (r.chance(0.1)) extra.push('Invulnerable:1b');
    if (r.chance(0.2)) extra.push(`Fire:${r.int(120)}s`);
    if (r.chance(0.2)) extra.push(`Health:${(1 + r.int(40)) / 2}f`);
    if (r.chance(0.5)) extra.push(`equipment:{head:{id:"minecraft:${r.pick(['iron_helmet', 'stone', 'diamond_helmet'])}",count:1},mainhand:{id:"minecraft:${solidItem}",count:${1 + r.int(5)}}}`);
    if (r.chance(0.2)) extra.push('Pose:{Head:[10.0f,20.0f,30.0f]}');
    add(standTag(n += 10, [b[0] + 0.5 + (r.int(7) - 3) / 2, b[1] + 1.0 + r.int(3), b[2] + 0.5 + (r.int(7) - 3) / 2], extra.join(',')));
  }
  if (entities.length) {
    args.entities = entities;
    args.item_entities = true;
    args.world_inventories = true;
    args.living_entities = true;
  }
}

// A book's saved stack: written book content (pages of plain or rich text,
// resolved or not) or writable book content, or none.
function bookSnbt(r, data, item, count) {
  const { text, compound, list, richComponent } = data;
  const page = () => (r.chance(0.6) ? text(`page ${r.int(100)}`) : richComponent(r, 2));
  const written = () => compound([['title', text('T')], ['author', text('A')], ['pages', list(Array.from({ length: r.int(6) }, page))],
    r.chance(0.3) && ['generation', String(r.int(4))], r.chance(0.5) && ['resolved', r.pick(['1b', '0b'])]]);
  const writable = () => compound([['pages', list(Array.from({ length: r.int(6) }, () => text(`line ${r.int(100)}`)))]]);
  let content = null;
  if (item === 'minecraft:written_book' && r.chance(0.9)) content = compound([['minecraft:written_book_content', written()]]);
  else if (item === 'minecraft:writable_book' && r.chance(0.9)) content = compound([['minecraft:writable_book_content', writable()]]);
  return compound([['id', text(item)], ['count', String(count)], content && ['components', content]]);
}

function addImports(base, placed, args, actions, last) {
  const data = require('./saved-data-fuzz');
  // Mostly saved data that Minecraft accepts and this engine reads, so experiments run.
  const r = { next: () => base.next(), int: (n) => base.int(n), pick: (list) => base.pick(list), chance: (p) => base.next() < p, hostile: Number(option('hostile', 0.1)), exotic: Number(option('exotic', 0.1)) };
  // Most requests save everything at the server's own data version; some at an older one (4189 in today's format, or the
  // format of an old version before 1.20.5's components, which the data fixer upgrades), some at a future one.
  const requestVersion = r.pick([4556, 4556, 4556, 4556, 4556, 4556, 4556, 4189, 99999, 'old', 'old']);
  const oldVersion = requestVersion === 'old' ? r.pick(data.OLD_VERSIONS) : null;
  const version = () => (r.chance(0.97) ? oldVersion || requestVersion : 4556);
  const old = (v) => v < 3818;
  if (r.next() < 0.6) {
    const entries = [];
    const last = new Map(placed.map((block) => [block.pos.join(','), block]));
    for (const block of last.values()) {
      const type = data.blockEntityType(block.id);
      if (!type || r.next() >= 0.8) continue;
      const v = version();
      const snbt = old(v) ? data.oldBlockEntity(r, type, v) : data.blockEntity(r, type, type);
      if (snbt) entries.push({ at: block.pos, data_version: v, snbt });
    }
    if (entries.length) args.block_entities = entries;
  }
  if (r.next() < 0.4) {
    const entities = [];
    for (let n = r.int(4); n > 0; n--) {
      const base = r.pick(placed).pos;
      if (r.chance(0.5)) {
        // An item frame on a side of a placed block, as the game hangs one.
        const [dx, dz, facing] = r.pick([[0, -1, 2], [0, 1, 3], [-1, 0, 4], [1, 0, 5]]);
        const block = [base[0] + dx, base[1], base[2] + dz];
        const at = [block[0] + 0.5 - dx * 0.46875, block[1] + 0.5, block[2] + 0.5 - dz * 0.46875];
        const frame = { id: r.pick(['minecraft:item_frame', 'minecraft:glow_item_frame']), block, facing };
        const v = version();
        entities.push({ at, data_version: v, snbt: old(v) ? data.oldEntityItem(r, entities.length, at, v, frame) : data.entityItem(r, entities.length, at, frame) });
        continue;
      }
      const at = [base[0] + 0.5, base[1] + 1.5, base[2] + 0.5];
      const v = version();
      entities.push({ at, data_version: v, snbt: old(v) ? data.oldEntityItem(r, entities.length, at, v) : data.entityItem(r, entities.length, at) });
    }
    if (entities.length) {
      args.entities = entities;
      if (r.chance(0.7)) args.item_entities = true;
      if (r.chance(0.5)) args.world_inventories = true;
    }
  }
  // Vehicles, paintings and armor stands in today's format only.
  if (!oldVersion && r.next() < 0.5) addVehicles(r, placed, args, actions, version);
  const supplied = (item, count, components) => {
    const v = version();
    return { data_version: v, snbt: old(v) ? data.oldStack(r, item, count) : data.stackOf(r, item, count, components) };
  };
  for (const action of actions) {
    // A book supplied to a lectern (one written or writable book, as the
    // verifier takes it): its stack now and then of two, of the other book,
    // or with random components.
    if (action.type === 'book' && action.present && r.next() < 0.5) {
      const item = r.pick(['minecraft:written_book', 'minecraft:written_book', 'minecraft:writable_book']);
      const saved = r.chance(0.9) ? item : r.pick(['minecraft:written_book', 'minecraft:writable_book']);
      const count = r.chance(0.95) ? 1 : 2;
      const v = version();
      Object.assign(action, { item, count: 1, stack: old(v) ? { data_version: v, snbt: data.oldStack(r, saved, count) } : { data_version: v, snbt: r.chance(0.8) ? bookSnbt(r, data, saved, count) : data.stackOf(r, saved, count, r.chance(0.8)) } });
    }
    if (action.type === 'insert' && r.next() < 0.6) action.stack = supplied(action.item, r.chance(0.9) ? action.count : action.count + 1, r.chance(0.8));
    if (action.type === 'drop' && r.next() < 0.6) action.stack = supplied(action.item, r.chance(0.9) ? action.count : action.count + 1, r.chance(0.8));
  }
  if (r.next() < 0.3) {
    const base = r.pick(placed).pos;
    actions.push({ tick: r.int(last + 1), type: 'insert', at: base, slot: r.int(5), item: 'minecraft:iron_ingot', count: 1 + r.int(3), stack: supplied('minecraft:iron_ingot', 1, true) });
  }
  // Fluid ticks join the saved tick queue (strict placement only).
  if (args.scheduled_ticks) {
    for (const block of placed.filter((b) => /water|lava/.test(b.id))) {
      if (r.next() < 0.5) args.scheduled_ticks.entries.push({ kind: 'fluid', id: r.pick(['minecraft:water', 'minecraft:flowing_water', 'minecraft:lava', 'minecraft:flowing_lava']), at: block.pos, delay: r.int(10) - 2, priority: r.int(9) - 4, sub_tick: String(7000 + args.scheduled_ticks.entries.length) });
    }
  }
}

// --- Running ---

// The Rust engine: core/ built, or the binary --rust names (an earlier
// commit's build, for what it refused).
function rustEngine() {
  if (option('rust', null) !== null) {
    const binary = path.resolve(option('rust'));
    return new Engine({ build: async () => binary });
  }
  execFileSync(rust().cargo, coreBuildArgs(), { cwd: path.join(ROOT, 'core'), env: rust().env, stdio: 'inherit' });
  const binary = path.join(ROOT, 'core', 'target', 'release', 'mcbench-core');
  return new Engine({ build: async () => binary });
}

// Runs one request on an engine into a folder: how long it took and the
// files, or why it failed.
async function runOn(engine, requestFile, commandsFile, folder) {
  fs.mkdirSync(folder, { recursive: true });
  const started = process.hrtime.bigint();
  let failed = null;
  await engine.verify(requestFile, commandsFile, folder).catch((error) => { failed = error.message.replace(/^The engine failed: /, ''); });
  const ms = Number(process.hrtime.bigint() - started) / 1e6;
  // A file past what a string holds (a long recorded experiment's report) is kept as bytes.
  const read = (name) => {
    const file = path.join(folder, name);
    if (!fs.existsSync(file)) return null;
    return fs.statSync(file).size > 256 * 1024 * 1024 ? fs.readFileSync(file) : fs.readFileSync(file, 'utf8');
  };
  return { ms, failed, report: read('report.json'), result: read('result.json'), progress: read('progress.json'), trace: read('entity-trace.txt') };
}

// A report or result without what differs between engines by nature; one
// held as bytes (too large for a string) as the SHA-256 of those bytes
// without the same fields.
function comparable(text) {
  if (Buffer.isBuffer(text)) {
    const hash = crypto.createHash('sha256');
    let at = 0;
    const skip = (open, close, first) => {
      const cuts = [];
      for (let i = text.indexOf(open); i >= 0; i = first ? -1 : text.indexOf(open, i + 1)) cuts.push([i + open.length, text.indexOf(close, i + open.length)]);
      return cuts;
    };
    const cuts = [...skip('"engine":"', '"', true), ...skip('"metrics":{', '}', false), ...skip(',"reportBytes":', ',', true)].sort((a, b) => a[0] - b[0]);
    let hashed = text.length;
    for (const [from, to] of cuts) {
      hash.update(text.subarray(at, from));
      hashed -= to - from;
      at = to;
    }
    hash.update(text.subarray(at));
    return `sha256 ${hash.digest('hex')} of ${hashed} bytes`;
  }
  return text
    .replace(/"engine":"[^"]*"/, '"engine":""')
    .replace(/"metrics":\{[^}]*\}/g, '"metrics":{}')
    .replace(/,"reportBytes":\d+/, '');
}

function firstDifference(a, b) {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  const window = (text) => JSON.stringify(text.slice(Math.max(0, i - 60), i + 100));
  return `at byte ${i} (java ${a.length} bytes, rust ${b.length}): java ${window(a)}, rust ${window(b)}`;
}

// The first line two traces differ at, with the tick (the number of the
// "time" lines before it) it is in.
function firstLineDifference(a, b) {
  const [x, y] = [a.split('\n'), b.split('\n')];
  let i = 0;
  while (i < x.length && i < y.length && x[i] === y[i]) i++;
  const tick = x.slice(0, i + 1).filter((line) => line.startsWith('time ')).length - 1;
  return `at line ${i + 1} (trace ${tick}, the state after that tick; java ${x.length} lines, rust ${y.length}):\n  java: ${x[i]}\n  rust: ${y[i]}`;
}

// How the Rust run differs from the Java run, or null.
function compare(java, rust) {
  if (!!java.failed !== !!rust.failed) return `java ${java.failed ? `failed: ${java.failed.slice(0, 300)}` : 'ran'}, rust ${rust.failed ? `failed: ${rust.failed.slice(0, 300)}` : 'ran'}`;
  const head = (reason) => reason.split(' <- ')[0].split(' @ ')[0];
  if (java.failed) {
    if (head(java.failed) !== head(rust.failed)) return `failures differ: java ${java.failed.slice(0, 300)}, rust ${rust.failed.slice(0, 300)}`;
  }
  // A finished run's last progress is its end; an ended run's is whichever was
  // due when the clock last allowed one.
  const finished = java.result !== null && JSON.parse(java.result).ok;
  for (const name of ['trace', 'result', 'report', ...(finished ? ['progress'] : [])]) {
    if ((java[name] === null) !== (rust[name] === null)) return `${name}.json: java ${java[name] === null ? 'missing' : 'written'}, rust ${rust[name] === null ? 'missing' : 'written'}`;
    if (java[name] === null) continue;
    const [a, b] = name === 'progress' || name === 'trace' ? [java[name], rust[name]] : [comparable(java[name]), comparable(rust[name])];
    if (a !== b) return `${name === 'trace' ? 'entity-trace.txt' : `${name}.json`} differs ${name === 'trace' ? firstLineDifference(a, b) : firstDifference(a, b)}`;
  }
  for (const [side, run] of [['java', java], ['rust', rust]]) {
    if (run.report !== null && run.result !== null && JSON.parse(run.result).ok && JSON.parse(run.result).reportBytes !== (Buffer.isBuffer(run.report) ? run.report.length : Buffer.byteLength(run.report))) return `${side}'s result.json reportBytes is not its report's size`;
  }
  return null;
}

async function main() {
  const placement = option('placement', 'both');
  if (!['both', ...PLACEMENT_MODES].includes(placement)) throw new Error(`Unknown --placement ${placement}`);
  const modes = placement === 'both' ? PLACEMENT_MODES : [placement];
  const seed = Number(option('seed', 1));
  let cases;
  if (process.argv.includes('--fixtures')) cases = fixtureCases(modes);
  else if (option('requests', null) !== null) {
    const folder = path.resolve(option('requests'));
    cases = fs.readdirSync(folder).sort().filter((name) => fs.existsSync(path.join(folder, name, 'request.json'))).map((name) => ({ name: `recorded/${name}`, recorded: path.join(folder, name) }));
  }
  else if (option('fuzz', null) !== null) cases = fuzzCases(Number(option('fuzz')), seed, option('families', null), process.argv.includes('--drops'));
  else cases = communityCases(Number(option('ticks', 60)), Number(option('every', 1)), modes);
  const match = new RegExp(option('match', ''));
  cases = cases.filter(({ name }) => match.test(name)).slice(0, Number(option('limit', Infinity)));

  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-parity-'));
  const keep = option('keep', null);
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const native = process.argv.includes('--native');
  // --stock: the second engine is the Java engine on Paper's own code paths
  // (MCBENCH_ENGINE_STOCK=1) instead of the Rust engine; --check runs the
  // first with its fast paths checked against Paper's (see scripts/core-parity.js).
  const javaEnv = engineEnv(option('first', process.argv.includes('--check') ? 'check' : '0'));
  const stockEnv = engineEnv('1');
  const javaEngine = (env, name) => {
    if (native) return new Engine({ spawnEngine: (binary, args, options) => spawn(binary, args, { ...options, env }) });
    fs.mkdirSync(path.join(scratch, name));
    return jvmEngine({ classpath, classes: path.join(scratch, name), env });
  };
  const java = option('base', null) !== null ? new Engine({ build: async () => path.resolve(option('base')) }) : javaEngine(javaEnv, 'classes');
  const rust = process.argv.includes('--stock') ? javaEngine(stockEnv, 'classes-stock') : rustEngine();
  const binaries = path.join(scratch, 'binaries');
  await Promise.all([
    java.warm({ classpath, binaries, folder: path.join(scratch, 'java'), minecraftVersion: minecraftLive.minecraftVersion }),
    rust.warm({ classpath, binaries, folder: path.join(scratch, 'rust'), minecraftVersion: minecraftLive.minecraftVersion }),
  ]);

  const verifiers = new Map();
  const verifierFor = (placementMode, config, seeded) => {
    const key = JSON.stringify([placementMode, config, seeded]);
    if (!verifiers.has(key)) verifiers.set(key, new MinecraftVerifier({ root: path.join(scratch, 'verifier'), placementMode, seed: seeded ? seed : null, ...config }));
    return verifiers.get(key);
  };
  const totals = { matched: 0, mismatched: 0, refused: 0, skipped: 0, invalid: 0 };
  const refusals = new Map();
  const olderVersions = new Map();
  const times = { rust: [], java: [], rustOnly: [], javaOnly: [] };
  for (const { name, placementMode, load, recorded } of cases) {
    let loaded;
    let request;
    let verifier;
    if (recorded) {
      request = JSON.parse(fs.readFileSync(path.join(recorded, 'request.json'), 'utf8'));
      if (!process.argv.includes('--unseeded')) request.seed = seed;
    } else try {
      loaded = load();
      verifier = verifierFor(placementMode, loaded.config, !process.argv.includes('--unseeded'));
      request = verifier.request(loaded.world, loaded.args);
    } catch (error) {
      totals.invalid++;
      console.log(`invalid   ${name}: ${String(error.message).slice(0, 200)}`);
      continue;
    }
    const folder = path.join(scratch, 'cases', crypto.randomUUID());
    fs.mkdirSync(folder, { recursive: true });
    const requestFile = path.join(folder, 'request.json');
    const commandsFile = path.join(folder, 'commands.txt');
    // Both engines write the entities' state after every tick (entity-trace.txt) when asked: the hidden state a
    // report does not show.
    fs.writeFileSync(requestFile, JSON.stringify({ ...(recorded ? request : verifier.transport(request)), ...(process.argv.includes('--no-trace') ? {} : { entity_trace: true }) }));
    if (recorded) fs.copyFileSync(path.join(recorded, 'commands.txt'), commandsFile);
    else fs.writeFileSync(commandsFile, `${importCommands(loaded.world, null, Y_OFFSET).commands.join('\n')}\n`);
    const rustRun = await runOn(rust, requestFile, commandsFile, path.join(folder, 'rust'));
    const refusal = rustRun.failed?.startsWith('unported:');
    if (refusal) {
      totals.refused++;
      const reason = rustRun.failed.slice('unported: '.length).replace(/ at -?\d+, -?\d+, -?\d+/g, '').replace(/\(.*\)/g, '').trim();
      refusals.set(reason, (refusals.get(reason) || 0) + 1);
      console.log(`refused   ${name}: ${rustRun.failed.slice(10, 200)}`);
    }
    if (!refusal || process.argv.includes('--timing') || process.argv.includes('--java-too')) {
      const javaRun = await runOn(java, requestFile, commandsFile, path.join(folder, 'java'));
      if (!refusal && process.argv.includes('--unseeded') && /"entityUUID"|"uuid":/.test(rustRun.report || '') ) {
        totals.skipped++;
        console.log(`skipped   ${name}: it makes entities, whose ids and UUIDs an unseeded run does not fix`);
      } else if (!refusal) {
        const difference = compare(javaRun, rustRun);
        times.rust.push(rustRun.ms);
        times.java.push(javaRun.ms);
        if (!difference) {
          totals.matched++;
          // The use inputs the matched experiments made, those a block took and those it did not (the same on both).
          for (const input of (javaRun.report && !Buffer.isBuffer(javaRun.report) && JSON.parse(javaRun.report).inputs) || []) {
            if (input.type === 'use') totals[input.applied ? 'used' : 'unused'] = (totals[input.applied ? 'used' : 'unused'] || 0) + 1;
          }
          const result = javaRun.result && JSON.parse(javaRun.result);
          // The older data versions of its saved data (the data fixer upgrades them), counted by version.
          const older = [...new Set([...fs.readFileSync(requestFile, 'utf8').matchAll(/"(?:block_)?data_version":(\d+)/g)].map((m) => Number(m[1])).filter((v) => v < 4556))].sort();
          for (const v of older) olderVersions.set(v, (olderVersions.get(v) || 0) + 1);
          console.log(`match     ${name} (${request.probes.length} probes, ${request.actions.length} inputs, ${request.duration + request.settle_ticks} ticks${request.scheduled_ticks ? `, ${request.scheduled_ticks.entries.length} saved ticks` : ''}${older.length ? `, data of ${older.join(', ')}` : ''}${javaRun.failed ? `, both failed: ${javaRun.failed.slice(0, 80)}` : result && !result.ok ? `, both ended: ${result.error.slice(0, 80)}` : ''}) rust ${rustRun.ms.toFixed(0)} ms, java ${javaRun.ms.toFixed(0)} ms`);
        } else {
          totals.mismatched++;
          console.log(`MISMATCH  ${name}: ${difference}`);
          if (keep) {
            const target = path.join(keep, name.replace(/[^\w.-]+/g, '_'));
            fs.mkdirSync(path.dirname(target), { recursive: true });
            fs.cpSync(folder, target, { recursive: true });
            console.log(`          kept ${target}`);
          }
        }
      }
    }
    // Unseeded, for the speed of each engine on the request the app makes.
    if (!refusal && !recorded && process.argv.includes('--timing')) {
      const plain = verifierFor(placementMode, loaded.config, false);
      fs.writeFileSync(requestFile, JSON.stringify(plain.transport(plain.request(loaded.world, loaded.args))));
      const [r, j] = [await runOn(rust, requestFile, commandsFile, path.join(folder, 'rust-plain')), await runOn(java, requestFile, commandsFile, path.join(folder, 'java-plain'))];
      times.rustOnly.push(r.ms);
      times.javaOnly.push(j.ms);
    }
    if (option('save', null)) fs.cpSync(folder, path.join(option('save'), name.replace(/[^\w.-]+/g, '_')), { recursive: true });
    fs.rmSync(folder, { recursive: true, force: true });
  }
  console.log(JSON.stringify(totals));
  for (const [reason, count] of [...refusals].sort((a, b) => b[1] - a[1])) console.log(`  ${count}\t${reason}`);
  if (olderVersions.size) console.log(`matched experiments with saved data of an older data version, by version: ${JSON.stringify(Object.fromEntries([...olderVersions].sort((a, b) => a[0] - b[0])))}`);
  const sum = (list) => list.reduce((a, b) => a + b, 0);
  if (times.rust.length) console.log(`seeded runs both engines answered: rust ${sum(times.rust).toFixed(0)} ms, java ${sum(times.java).toFixed(0)} ms over ${times.rust.length} experiments`);
  if (times.rustOnly.length) console.log(`unseeded (the app's requests): rust ${sum(times.rustOnly).toFixed(0)} ms, java ${sum(times.javaOnly).toFixed(0)} ms over ${times.rustOnly.length} experiments`);
  await Promise.all([java.stop(), rust.stop()]);
  fs.rmSync(scratch, { recursive: true, force: true });
  // Only the Rust engine refuses: the Java engine does not run a refused
  // request. A case whose request could not be made (invalid) errored.
  console.log(`${totals.matched} matched, ${totals.mismatched} differed, 0 refused by both, ${totals.refused} refused by the Rust engine alone, ${totals.invalid} errored${totals.skipped ? `, ${totals.skipped} skipped (entities, unseeded)` : ''}`);
  // A run that compared nothing proves nothing.
  if (totals.mismatched || totals.invalid || !totals.matched) process.exitCode = 1;
}

module.exports = { main, fuzzCases };
