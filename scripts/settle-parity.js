#!/usr/bin/env node
'use strict';

// Does a settle without lighting (TICK ... nolight, which shot-world.js
// asks for) leave the world the stock settle leaves? For each world the
// settled blueprint the engine writes (fresh placement, SETTLE_TICKS ticks,
// write) is compared byte for byte between the Rust engine with the ticks'
// final light, the Rust engine without it, and the Java engine, which does
// not light differently (its TICK ignores the extra word). Worlds: stored
// conversations, one-block edits of them, and random worlds of dynamic
// blocks (water, lava, sand, redstone, pistons, plants that read light...)
// and of every block in the registry with random states.
// With --snapshots each world is also settled at several ticks in one run of
// each engine, the world written after each, and every snapshot is compared
// with a separate fresh run to that many ticks (and the Rust engine's with the
// Java engine's).
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/settle-parity.js [--fuzz 200] [--edits 6] [--seed 1] [--snapshots] [conversation id]...
// Wrap in the native-engine lock: until mkdir /tmp/mcbench-native.lock; do sleep 30; done

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { Engine, coreEngine } = require('../server/engine');
const { minecraftLive } = require('../server/minecraft-live');
const { importCommands } = require('../server/live-placement');
const { getBlockRegistry } = require('../server/assets');
const { DEFAULT_SETTLE_TICKS: SETTLE_TICKS } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const store = require('../server/store');

const DYNAMIC = ['water', 'lava', 'sand', 'gravel', 'red_sand', 'white_concrete_powder', 'redstone_wire', 'redstone_torch', 'redstone_lamp', 'repeater', 'comparator', 'piston', 'sticky_piston',
  'observer', 'tnt', 'oak_leaves', 'oak_sapling', 'wheat', 'carrots', 'red_mushroom', 'brown_mushroom', 'daylight_detector', 'frosted_ice', 'ice', 'fire', 'cactus', 'sugar_cane', 'vine', 'kelp',
  'bamboo', 'hopper', 'dropper', 'dispenser', 'lever', 'stone_button', 'oak_pressure_plate', 'rail', 'powered_rail', 'detector_rail', 'activator_rail', 'anvil', 'dripstone_block', 'pointed_dripstone',
  'scaffolding', 'snow', 'powder_snow', 'farmland', 'dirt', 'grass_block', 'mud', 'lightning_rod', 'bubble_column', 'seagrass', 'big_dripleaf', 'composter', 'cauldron', 'sculk_sensor', 'target',
  'slime_block', 'honey_block', 'glowstone', 'torch', 'lantern', 'campfire', 'magma_block', 'oak_door', 'oak_trapdoor', 'oak_fence_gate', 'note_block', 'jukebox', 'chest', 'barrel', 'lectern'];

function random(seed) {
  let state = seed >>> 0;
  return () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 2 ** 32; };
}

function randomWorld(registry, names, rnd, kind) {
  const world = new World();
  const size = [18 + Math.floor(rnd() * 10), 8 + Math.floor(rnd() * 8), 18 + Math.floor(rnd() * 10)];
  const pick = (list) => list[Math.floor(rnd() * list.length)];
  const state = (name) => {
    const [properties] = registry[name];
    const states = {};
    for (const [key, values] of Object.entries(properties)) if (rnd() < 0.6) states[key] = pick(values);
    return states;
  };
  const density = 0.15 + rnd() * 0.35;
  // A few kinds of block per world, so a block the Rust engine refuses costs one world, not all.
  const kinds = Array.from({ length: 2 + Math.floor(rnd() * 5) }, () => (kind === 'chaos' ? pick(names) : pick(DYNAMIC)));
  world.fill([0, 0, 0], [size[0], 0, size[2]], 'minecraft:stone');
  for (let x = 0; x <= size[0]; x += 1) for (let y = 1; y <= size[1]; y += 1) for (let z = 0; z <= size[2]; z += 1) {
    if (rnd() > density) continue;
    const name = pick(kinds);
    if (!registry[name]) continue;
    world.set(x, y, z, `minecraft:${name}`, state(name));
  }
  return world.finalize();
}

// The settled blueprint, or the error the engine answers with (Minecraft
// reports an error on some random worlds, and goes on from it).
async function settled(engine, commandsFile, scratch, light) {
  const file = (name) => path.join(scratch, name);
  try {
    await engine.place(commandsFile, file('placed.bin'), { fresh: true });
    await engine.tick(SETTLE_TICKS, file('patch.bin'), file('entities.json'), { light });
    await engine.place(file('none.txt'), file('settled.bin'), { fresh: false });
    return fs.readFileSync(file('settled.bin'));
  } catch (error) {
    if (/unported:/.test(error.message)) throw error;
    return Buffer.from(`error: ${error.message}`);
  }
}

const SNAPSHOT_TICKS = [0, 1, 7, 30, 100, 210];

// The world after each of `ticks` from one run of `engine`: placed, ticked up to the next, written.
async function snapshots(engine, commandsFile, scratch, ticks) {
  const file = (name) => path.join(scratch, name);
  const out = new Map();
  try {
    await engine.place(commandsFile, file('placed.bin'), { fresh: true });
    let at = 0;
    for (const tick of ticks) {
      if (tick > at) await engine.tick(tick - at, file('patch.bin'), file('entities.json'), { light: false });
      at = tick;
      if (tick === 0) out.set(0, fs.readFileSync(file('placed.bin')));
      else {
        await engine.place(file('none.txt'), file('snapshot.bin'), { fresh: false });
        out.set(tick, fs.readFileSync(file('snapshot.bin')));
      }
    }
  } catch (error) {
    if (/unported:/.test(error.message)) throw error;
    for (const tick of ticks) out.set(tick, Buffer.from(`error: ${error.message}`));
  }
  return out;
}

async function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback) => { const at = args.indexOf(name); return at === -1 ? fallback : Number(args.splice(at, 2)[1]); };
  const fuzz = option('--fuzz', 200);
  const edits = option('--edits', 6);
  const seed = option('--seed', 1);
  const withSnapshots = args.includes('--snapshots');
  const ids = args.filter((arg) => arg !== '--snapshots');
  const registry = await getBlockRegistry();
  const names = Object.keys(registry);
  // The Rust engine, and the native Java engine as its reference.
  const engines = { core: coreEngine(), java: new Engine() };
  const classpath = await minecraftLive.ensurePaper();
  await engines.core.warm({ folder: path.join(minecraftLive.engineFolder, 'parity-core') });
  await engines.java.warm({ classpath: classpath.map((file) => path.join(minecraftLive.paperDir, file)), binaries: minecraftLive.engineFolder, folder: path.join(minecraftLive.engineFolder, 'parity'), minecraftVersion: minecraftLive.minecraftVersion });
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-parity-'));
  fs.writeFileSync(path.join(scratch, 'none.txt'), '');
  const tally = { worlds: 0, identical: 0, different: 0, refused: 0, sameError: 0, javaDiffers: 0, javaDiffersInErrorText: 0, snapshotWorlds: 0, snapshotsIdentical: 0, snapshotsDifferent: 0, snapshotErrors: 0, worldsMatched: 0, worldsDiffered: 0 };
  const rnd = random(seed);

  async function compare(label, world) {
    tally.worlds += 1;
    const commandsFile = path.join(scratch, 'commands.txt');
    fs.writeFileSync(commandsFile, importCommands(world, null, minecraftLive.yOffset).commands.map((command) => `${command}\n`).join(''));
    let stock;
    let quick;
    try {
      stock = await settled(engines.core, commandsFile, scratch, true);
      quick = await settled(engines.core, commandsFile, scratch, false);
    } catch (error) {
      if (!/unported:/.test(error.message)) throw error;
      tally.refused += 1;
      return;
    }
    const java = await settled(engines.java, commandsFile, scratch, true);
    // What differed before this world, to tell whether anything of it differs.
    const before = tally.different + tally.javaDiffers + tally.snapshotsDifferent;
    if (withSnapshots) await compareSnapshots(label, commandsFile);
    const digest = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex').slice(0, 12);
    const reported = stock.subarray(0, 7).toString() === 'error: ';
    if (!stock.equals(java)) {
      tally[reported ? 'javaDiffersInErrorText' : 'javaDiffers'] += 1;
      if (!reported) console.log(`JAVA DIFFERENT ${label}: rust ${digest(stock)}, java ${digest(java)}`);
    }
    if (stock.equals(quick)) {
      tally.identical += 1;
      if (reported) tally.sameError += 1;
    } else {
      tally.different += 1;
      console.log(`DIFFERENT ${label}: rust ${digest(stock)}, rust nolight ${digest(quick)}, java ${digest(java)}`);
    }
    tally[tally.different + tally.javaDiffers + tally.snapshotsDifferent > before ? 'worldsDiffered' : 'worldsMatched'] += 1;
  }

  // Each snapshot of one run is a separate run's, on both engines, and the engines agree.
  async function compareSnapshots(label, commandsFile) {
    tally.snapshotWorlds += 1;
    const separate = async (engine, tick) => (await snapshots(engine, commandsFile, scratch, [tick])).get(tick);
    const runs = [engines.core, engines.java];
    const together = [];
    for (const engine of runs) together.push(await snapshots(engine, commandsFile, scratch, SNAPSHOT_TICKS));
    for (const tick of SNAPSHOT_TICKS) {
      const alone = [];
      for (const engine of runs) alone.push(await separate(engine, tick));
      // An error Minecraft reports carries what differs run to run (ids, times): such a world is counted apart.
      if (together.some((run) => run.get(tick).subarray(0, 7).toString() === 'error: ')) {
        tally.snapshotErrors += 1;
        continue;
      }
      const same = together[0].get(tick).equals(alone[0]) && together[1].get(tick).equals(alone[1]);
      if (same) tally.snapshotsIdentical += 1;
      else {
        tally.snapshotsDifferent += 1;
        console.log(`SNAPSHOT DIFFERENT ${label} tick ${tick}: rust run ${together[0].get(tick).equals(alone[0])}, java run ${together[1].get(tick).equals(alone[1])}`);
        if (process.env.SETTLE_PARITY_KEEP) fs.copyFileSync(commandsFile, path.join(process.env.SETTLE_PARITY_KEEP, `${label.replace(/[^a-z0-9]+/gi, '-')}.txt`));
      }
      if (!together[0].get(tick).equals(together[1].get(tick))) tally.javaDiffers += 1;
    }
  }

  for (const id of ids) {
    const conversation = store.getConversation(id);
    await compare(`${id.slice(0, 8)} ${conversation.title}`, conversation.world);
    const positions = [];
    conversation.world.forEach((x, y, z) => positions.push([x, y, z]));
    for (let i = 0; i < edits; i += 1) {
      const [x, y, z] = positions[Math.floor(rnd() * positions.length)];
      const edited = conversation.world.clone();
      if (i % 2 === 0) edited.set(x, y, z, 'minecraft:air');
      else edited.set(x, y + 1, z, 'minecraft:water', { level: '0' });
      await compare(`${id.slice(0, 8)} edit ${i} at ${x},${y},${z}`, edited.finalize());
    }
    console.log(`${id.slice(0, 8)} ${conversation.title}: ${JSON.stringify(tally)}`);
  }
  for (let i = 0; i < fuzz; i += 1) {
    await compare(`fuzz ${i} (${i % 2 ? 'dynamic' : 'chaos'})`, randomWorld(registry, names, rnd, i % 2 ? 'dynamic' : 'chaos'));
    if ((i + 1) % 50 === 0) console.log(`fuzz ${i + 1}: ${JSON.stringify(tally)}`);
  }
  console.log(JSON.stringify(tally));
  fs.rmSync(scratch, { recursive: true, force: true });
  await Promise.all([engines.core.stop(), engines.java.stop()]);
  store.close();
  // Only the Rust engine refuses: the Java engine does not run a refused
  // world. A world that throws ends the run, so none is counted as errored.
  console.log(`${tally.worldsMatched} matched, ${tally.worldsDiffered} differed, 0 refused by both, ${tally.refused} refused by the Rust engine alone, 0 errored`);
  // A run that compared nothing proves nothing.
  process.exit(tally.worldsDiffered || !tally.worldsMatched ? 1 : 0);
}

main().catch((error) => { console.error(error); process.exit(1); });
