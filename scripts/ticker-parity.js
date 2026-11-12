'use strict';

// Checks the engine (McbenchExperiment on McbenchLevel) against Paper's
// verifier on real builds. For each community redstone build in
// the corpus MCBENCH_CORPUS names (or the builds named), it makes one experiment: every
// redstone component probed, each lever toggled and each button pressed,
// every tick (or every Nth) sampled. The experiment runs in the isolated Paper verifier and
// in the engine, and the two reports are compared field by field (all but
// the engine name, version and timings).
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/ticker-parity.js \
//     [--limit N] [--match regex] [--ticks N] [--every N] [--placement updates|strict]
//     [--report file.json] [--keep folder] [--jvm] [--seed N] [--fixtures]
//
// --fixtures runs scripts/verifier-fixtures.js (worlds and requests that use
// the verifier's imports, inputs and censuses, which the community builds
// do not) instead of the community builds. Each fixture's own request is
// compared as written (--ticks and --every do not apply); a fixture's
// placementMode replaces --placement, and fixtures run grouped by mode.
//
// --jvm runs the engine's classes on GraalVM's JVM instead of building the
// native program, for checking a change quickly. --seed fixes both worlds'
// random draws (dispensers, droppers, drops), which Paper otherwise leaves
// unseeded; entities' UUIDs stay random on both sides, so reports are
// compared without them, their entity lists in a canonical order.
//
// --keep saves both reports of each build whose reports differ.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const os = require('os');
const ROOT = path.join(__dirname, '..');
if (!process.env.MCBENCH_DATA_DIR || fs.realpathSync(path.resolve(process.env.MCBENCH_DATA_DIR)) === fs.realpathSync(path.join(ROOT, 'data'))) {
  throw new Error('Set MCBENCH_DATA_DIR to a scratch copy of the data folder');
}
const { minecraftLive } = require('../server/minecraft-live');
const { jvmEngine } = require('../server/engine');
const { MinecraftVerifier } = require('../server/minecraft-verify');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');
const PLACEMENT_MODES = ['updates', 'strict'];

const DATA_DIR = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(ROOT, 'data'));
const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;
const CONTAINERS = /chest$|barrel|hopper$|dropper|dispenser|furnace$|smoker|shulker_box/;

function communityBuilds() {
  const builds = new Map(corpusArtifacts());
  return [...builds].sort(([a], [b]) => a.localeCompare(b));
}

// Every redstone component probed, each lever toggled twice and each button
// pressed; up to 16 containers probed with their inventories, items put into
// up to 8 of them and the first drained; the world's inventories observed.
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
  // Items thrown onto the first hopper, as an item stream would arrive.
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

// The first place two values differ, as a path.
function difference(a, b, at = '') {
  if (a === b) return null;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) return `${at}: ${JSON.stringify(a)?.slice(0, 200)} vs ${JSON.stringify(b)?.slice(0, 200)}`;
  for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
    const found = difference(a[key], b[key], `${at}.${key}`);
    if (found) return found;
  }
  return null;
}

// A report without what differs between runs of the same world: the
// engine's name and timings, and entities' random UUIDs (entity lists sorted
// by UUID are put in a canonical order).
function comparable(report) {
  const { engine, minecraftVersion, metrics, ...rest } = report;
  return JSON.parse(JSON.stringify(rest), (key, value) => {
    if (Array.isArray(value) && value.length && value.every((entry) => entry && typeof entry === 'object' && 'uuid' in entry)) {
      return value.map(({ uuid, ...entry }) => entry).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
    }
    if (value && typeof value === 'object' && !Array.isArray(value) && 'entityUUID' in value) {
      const { entityUUID, ...entry } = value;
      return entry;
    }
    return value;
  });
}

// The cases to run: each has a name, the placement mode its Paper verifier
// uses and a load() giving its world and request.
function communityCases(ticks, every, placementMode) {
  return communityBuilds().map(([name, file]) => ({
    name, placementMode,
    load() {
      const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
      const world = new World();
      for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
      world.finalize();
      return { world, args: experiment(artifact, ticks, every) };
    },
  }));
}

function fixtureCases(placementMode) {
  const seen = new Set();
  return require('./verifier-fixtures').map((fixture) => {
    const { name, world, args } = fixture;
    if (typeof name !== 'string' || !name) throw new Error('A fixture needs a name');
    if (seen.has(name)) throw new Error(`Duplicate fixture ${name}`);
    seen.add(name);
    if (typeof world !== 'function') throw new Error(`Fixture ${name} needs a world function`);
    if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error(`Fixture ${name} needs an args object`);
    if (fixture.placementMode !== undefined && !PLACEMENT_MODES.includes(fixture.placementMode)) throw new Error(`Fixture ${name} has unknown placementMode ${fixture.placementMode}`);
    return {
      name, placementMode: fixture.placementMode ?? placementMode,
      load() {
        const built = world();
        if (!(built instanceof World)) throw new Error(`Fixture ${name} world() did not return a World`);
        return { world: built, args };
      },
    };
  });
}

async function main() {
  const limit = Number(option('limit', Infinity));
  const match = option('match', '');
  const ticks = Number(option('ticks', 60));
  const keep = option('keep', null);
  const placement = option('placement', 'updates');
  if (!PLACEMENT_MODES.includes(placement)) throw new Error(`Unknown --placement ${placement}`);
  const all = process.argv.includes('--fixtures') ? fixtureCases(placement) : communityCases(ticks, Number(option('every', 1)), placement);
  // Grouped by placement mode (a stable sort), so one verifier serves each mode in turn.
  const cases = all.filter(({ name }) => new RegExp(match).test(name)).slice(0, limit).sort((a, b) => PLACEMENT_MODES.indexOf(a.placementMode) - PLACEMENT_MODES.indexOf(b.placementMode));
  const seed = option('seed', null);
  let verifier = null;
  const scratch = path.join(DATA_DIR, 'ticker-parity', 'engine');
  fs.mkdirSync(scratch, { recursive: true });
  if (process.argv.includes('--jvm')) {
    const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
    minecraftLive.engine = jvmEngine({ classpath, classes: fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-classes-')) });
  }
  await minecraftLive.warmEngine();
  const results = [];
  for (const { name, placementMode, load } of cases) {
    if (verifier?.placementMode !== placementMode) {
      await verifier?.stop();
      verifier = new MinecraftVerifier({ root: path.join(DATA_DIR, 'ticker-parity', 'paper'), placementMode, seed: seed === null ? null : Number(seed) });
    }
    const { world, args } = load();
    const request = verifier.request(world, args);
    const result = { name, blocks: world.size(), probes: request.probes.length, actions: request.actions.length };
    try {
      const folder = path.join(scratch, crypto.randomUUID());
      fs.mkdirSync(folder);
      const requestFile = path.join(folder, 'request.json');
      fs.writeFileSync(requestFile, JSON.stringify(verifier.transport(request)));
      const commandsFile = path.join(folder, 'commands.txt');
      fs.writeFileSync(commandsFile, `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
      let started = Date.now();
      let engineError = null;
      await minecraftLive.engine.verify(requestFile, commandsFile, folder).catch((error) => { engineError = error; });
      result.engineMs = Date.now() - started;
      started = Date.now();
      const paperReport = path.join(folder, 'paper-report.json');
      // A request the experiment refuses before it starts (MCBENCH_ERROR
      // from the plugin) must fail on the engine with the same message (the
      // JVM engine adds the error's frames after " @ "; the native program has none).
      let paperError = null;
      const paper = await verifier.run(world, args, { reportPath: paperReport }).catch((error) => { paperError = error; return null; });
      if (paperError) {
        const refused = /MCBENCH_ERROR (.*)$/s.exec(String(paperError.message || paperError));
        if (!refused) throw paperError;
        const engineMessage = engineError && /^The engine failed: java\.io\.IOException: (.*?)(?: @ |$)/s.exec(engineError.message);
        result.identical = !!engineMessage && engineMessage[1] === refused[1].trim();
        result.refused = result.identical;
        if (!result.identical) result.difference = `Paper refused (${refused[1].trim().slice(0, 300)}), engine ${engineError ? `failed: ${engineError.message.slice(0, 300)}` : 'ran'}`;
        result.paperMs = Date.now() - started;
        fs.rmSync(folder, { recursive: true, force: true });
        results.push(result);
        console.log(`${name}: ${result.blocks} blocks, ${result.probes} probes, ${result.actions} inputs, engine ${result.engineMs} ms: ${result.identical ? 'identical refusal' : `differs: ${result.difference}`}`);
        continue;
      }
      result.paperMs = Date.now() - started;
      result.paperSimulationMs = paper.timings?.simulationMs;
      if (engineError) {
        result.unsupported = engineError.message.replace('The engine failed: ', '').slice(0, 2000);
      } else {
        const engine = JSON.parse(fs.readFileSync(path.join(folder, 'report.json'), 'utf8'));
        const native = JSON.parse(fs.readFileSync(paperReport, 'utf8'));
        result.difference = difference(comparable(engine), comparable(native));
        result.identical = !result.difference;
        if (keep && !result.identical) {
          fs.mkdirSync(path.join(keep, name), { recursive: true });
          fs.copyFileSync(path.join(folder, 'report.json'), path.join(keep, name, 'engine.json'));
          fs.copyFileSync(paperReport, path.join(keep, name, 'paper.json'));
        }
      }
      fs.rmSync(folder, { recursive: true, force: true });
    } catch (error) {
      result.error = String(error.message || error).slice(0, 1000);
    }
    results.push(result);
    const outcome = result.error ? `error: ${result.error}` : result.unsupported ? `not modelled: ${result.unsupported}` : result.identical ? 'identical' : `differs at ${result.difference}`;
    console.log(`${name}: ${result.blocks} blocks, ${result.probes} probes, ${result.actions} inputs, engine ${result.engineMs} ms, Paper ${result.paperMs} ms (${result.paperSimulationMs} ms ticking): ${outcome}`);
  }
  await Promise.all([verifier?.stop(), minecraftLive.engine.stop()]);
  const report = option('report', null);
  if (report) fs.writeFileSync(report, JSON.stringify(results, null, 2));
  // The Rust engine's refusal ("unported: ...") of what Paper ran is counted
  // apart, as scripts/core-parity.js counts it; any other engine failure where
  // Paper ran is a difference.
  const kind = (result) => {
    if (result.error) return 'errored';
    if (result.unsupported) return result.unsupported.startsWith('unported:') ? 'refusedRust' : 'differed';
    if (result.refused) return 'refusedBoth';
    return result.identical ? 'matched' : 'differed';
  };
  const totals = { matched: 0, differed: 0, refusedBoth: 0, refusedRust: 0, errored: 0 };
  for (const result of results) totals[kind(result)] += 1;
  console.log(`\n${totals.matched} matched, ${totals.differed} differed, ${totals.refusedBoth} refused by both, ${totals.refusedRust} refused by the engine alone, ${totals.errored} errored`);
  // A run that compared nothing proves nothing.
  return !totals.differed && !totals.errored && totals.matched + totals.differed > 0;
}

main().then((passed) => process.exit(passed ? 0 : 1), (error) => { console.error(error); process.exit(1); });
