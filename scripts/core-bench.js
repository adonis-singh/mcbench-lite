'use strict';

// Speed benchmark of the engines on the same work: for each item (a community
// build of the corpus MCBENCH_CORPUS names, or a saved world of the data folder) and each
// engine, three timed requests: a fresh placement, live ticks (the levers
// toggled and the buttons pressed, as scripts/core-parity.js does, ticked in
// requests of --chunk), and a VERIFY request (the community experiment of
// scripts/core-verify-parity.js, unseeded as the app sends it; --seeded and
// --trace send the seed and the entity trace the parity harness does, which
// makes the engines' reports comparable, hashed and checked). Rounds are run engine after engine in a
// rotating order; the first --warmup rounds are dropped, the rest give each
// item's medians. The engines' outputs are hashed and must agree.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-bench.js
//     [--engines java,rust]        java (the native program, fast paths), stock
//                                  (the same with MCBENCH_ENGINE_STOCK=1), rust
//                                  (core/target/release), NAME=PATH (a Rust binary)
//     [--rounds 5] [--warmup 1] [--ticks 1000] [--chunk 100] [--verify-ticks 100]
//     [--match regex] [--limit N] [--corpus yes|no] [--worlds id,id|all|none]
//     [--phases place,tick,verify] [--seeded] [--trace] [--nolight] [--graceful] [--json out.json]
//     [--fuzz N --seed S --ticks T [--drops] [--flow] [--shift]]  the fuzzer's worlds of
//                                  scripts/core-parity.js instead, run untimed (to record a profile)
//   node scripts/core-bench.js --diff before.json after.json

const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { execFileSync, spawn } = require('child_process');
const ROOT = path.join(__dirname, '..');
const { minecraftLive } = require('../server/minecraft-live');
const { Engine, engineEnv, coreBuildArgs, rust } = require('../server/engine');
const { MinecraftVerifier } = require('../server/minecraft-verify');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;
const CONTAINERS = /chest$|barrel|hopper$|dropper|dispenser|furnace$|smoker|shulker_box/;
const PHASES = option('phases', 'place,tick,verify').split(',');

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};
const sha = (...buffers) => {
  const hash = crypto.createHash('sha256');
  for (const buffer of buffers) hash.update(buffer);
  return hash.digest('hex').slice(0, 16);
};

// --- Items: { name, kind, blocks, world, components } ---

function communityBuilds() {
  const builds = new Map(corpusArtifacts());
  return [...builds].sort(([a], [b]) => a.localeCompare(b));
}

// The blocks an experiment and the inputs look at: components and containers.
function relevant(world) {
  const blocks = [];
  let first = null;
  world.forEach((x, y, z, entry) => {
    first ||= { id: entry.id, pos: [x, y, z] };
    if (COMPONENTS.test(entry.id) || CONTAINERS.test(entry.id)) blocks.push({ id: entry.id, pos: [x, y, z] });
  });
  return { blocks, first };
}

function loadItems() {
  const match = new RegExp(option('match', ''));
  const items = [];
  if (option('corpus', 'yes') === 'yes') {
    for (const [name, file] of communityBuilds()) {
      if (!match.test(name)) continue;
      items.push({
        name, kind: 'community',
        load() {
          const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
          const world = new World();
          for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
          world.finalize();
          return { world, blocks: artifact.blocks, first: artifact.blocks[0] };
        },
      });
    }
  }
  const worlds = option('worlds', 'none');
  if (worlds !== 'none') {
    const store = require('../server/store');
    const dataDir = process.env.MCBENCH_DATA_DIR;
    const ids = worlds === 'all' ? fs.readdirSync(path.join(dataDir, 'worlds')).sort() : worlds.split(',');
    for (const id of ids) {
      if (!match.test(id)) continue;
      // A world folder with no chat of the database (left by one removed) is not a world to run.
      if (worlds === 'all' && !store.getConversation(id)) continue;
      items.push({
        name: id.slice(0, 8), kind: 'world',
        load() {
          const world = store.getConversation(id).world;
          world.finalize();
          return { world, ...(() => { const r = relevant(world); return { blocks: r.blocks, first: r.first }; })() };
        },
      });
    }
  }
  return items.slice(0, Number(option('limit', Infinity)));
}

// scripts/core-verify-parity.js's experiment of a community build.
function experiment(blocks, first, ticks) {
  const probes = [];
  const actions = [];
  const containers = blocks.filter((block) => CONTAINERS.test(block.id)).slice(0, 16).map((block) => block.pos);
  probes.push(...containers);
  for (const block of blocks) {
    if (!COMPONENTS.test(block.id)) continue;
    if (probes.length < 256 && !containers.includes(block.pos)) probes.push(block.pos);
    if (block.id === 'minecraft:lever') actions.push({ tick: 2, type: 'toggle', at: block.pos }, { tick: Math.floor(ticks / 2), type: 'toggle', at: block.pos });
    else if (block.id.endsWith('_button')) actions.push({ tick: 4, type: 'press', at: block.pos });
  }
  containers.slice(0, 8).forEach((at, index) => actions.push({ tick: 3 + index, type: 'insert', at, slot: 0, count: 16, item: 'minecraft:cobblestone' }));
  const hopper = blocks.find((block) => block.id === 'minecraft:hopper');
  if (hopper) actions.push({ tick: 6, type: 'drop', at: [hopper.pos[0] + 0.5, hopper.pos[1] + 1.5, hopper.pos[2] + 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] });
  if (containers.length) actions.push({ tick: ticks - 10, type: 'drain', at: containers[0] });
  if (!probes.length) probes.push(first.pos);
  return {
    probes, actions, samples: Array.from({ length: Math.floor(ticks / 1) + 1 }, (_, index) => index),
    ...(containers.length ? { inventory_probes: containers, inventory_missing: 'report', world_inventories: true } : {}),
    item_entities: true, living_entities: true,
  };
}

// --- Engines ---

function rustBinary(spec) {
  if (spec !== 'rust') return spec;
  execFileSync(rust().cargo, coreBuildArgs(), { cwd: path.join(ROOT, 'core'), env: rust().env, stdio: 'inherit' });
  return path.join(ROOT, 'core', 'target', 'release', 'mcbench-core');
}

function makeEngine(spec) {
  if (spec === 'java') return new Engine();
  if (spec === 'stock') {
    const env = engineEnv('1');
    return new Engine({ spawnEngine: (binary, args, options) => spawn(binary, args, { ...options, env }) });
  }
  const [name, file] = spec.includes('=') ? spec.split('=') : [spec, spec];
  const binary = rustBinary(file);
  return new Engine({ build: async () => binary });
}
const labelOf = (spec) => (spec.includes('=') ? spec.split('=')[0] : spec);

// --- Runs ---

// The refusals ("unported: ...") of a run by reason, positions taken out.
const refusals = new Map();
const countRefusal = (reason) => {
  if (!reason || !/^unported:/.test(reason)) return;
  const key = reason.replace(/-?\d+, -?\d+, -?\d+/g, 'x, y, z').replace(/(entity) -?\d+/g, '$1 n');
  refusals.set(key, (refusals.get(key) || 0) + 1);
};
function printRefusals() {
  if (!refusals.size) return;
  console.log('\nrefusals:');
  for (const [reason, count] of [...refusals].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) console.log(`${String(count).padStart(6)}  ${reason}`);
}

const comparable = (text) => text.replace(/"engine":"[^"]*"/, '"engine":""').replace(/"metrics":\{[^}]*\}/g, '"metrics":{}').replace(/,"reportBytes":\d+/, '');

async function timed(fn) {
  const started = process.hrtime.bigint();
  const value = await fn();
  return { ms: Number(process.hrtime.bigint() - started) / 1e6, value };
}

// One engine on one item: { place, tick, verify } in ms and { hashes }, or
// { failed } with the phase and reason (a refusal is "unported: ...").
async function runItem(engine, item, prepared, scratch) {
  const out = { times: {}, hashes: {} };
  const file = (name) => path.join(scratch, name);
  const inputs = (kind) => {
    const toggles = [];
    for (const block of prepared.blocks) {
      const at = [block.pos[0], block.pos[1] + Y_OFFSET, block.pos[2]];
      if (kind === 'start' && block.id === 'minecraft:lever') toggles.push(['toggle', ...at, 0]);
      if (kind === 'start' && block.id.endsWith('_button')) toggles.push(['press', ...at, 0]);
      if (kind === 'middle' && block.id === 'minecraft:lever') toggles.push(['toggle', ...at, 0]);
    }
    return toggles;
  };
  try {
    if (PHASES.includes('place') || PHASES.includes('tick')) {
      const placed = await timed(() => engine.place(prepared.commandsFile, file('blueprint.bin'), { fresh: true }));
      out.times.place = placed.ms;
      out.hashes.place = sha(fs.readFileSync(file('blueprint.bin')));
    }
    if (PHASES.includes('tick')) {
      const total = Number(option('ticks', 1000));
      const chunk = Number(option('chunk', 100));
      let spent = 0;
      const hash = crypto.createHash('sha256');
      for (let done = 0; done < total; done += chunk) {
        if (done === 0) for (const input of inputs('start')) await engine.input(...input);
        if (done === Math.floor(total / 2 / chunk) * chunk) for (const input of inputs('middle')) await engine.input(...input);
        const n = Math.min(chunk, total - done);
        const ticked = await timed(() => engine.tick(n, file('patch.bin'), file('entities.json'), { light: !process.argv.includes('--nolight') }));
        spent += ticked.ms;
        hash.update(fs.readFileSync(file('patch.bin')));
        hash.update(fs.readFileSync(file('entities.json')));
      }
      out.times.tick = spent;
      out.hashes.tick = hash.digest('hex').slice(0, 16);
    }
  } catch (error) {
    return { failed: error.message.replace(/^The engine failed: /, '') };
  }
  if (PHASES.includes('verify')) {
    const folder = file('verify');
    fs.rmSync(folder, { recursive: true, force: true });
    fs.mkdirSync(folder, { recursive: true });
    try {
      const verified = await timed(() => engine.verify(prepared.requestFile, prepared.commandsFile, folder));
      out.times.verify = verified.ms;
      const read = (name) => (fs.existsSync(path.join(folder, name)) ? comparable(fs.readFileSync(path.join(folder, name), 'utf8')) : '');
      if (process.argv.includes('--seeded')) out.hashes.verify = sha(read('report.json'), read('result.json'), read('entity-trace.txt'));
    } catch (error) {
      out.verifyFailed = error.message.replace(/^The engine failed: /, '');
    }
  }
  return out;
}

async function main() {
  const specs = option('engines', 'java,rust').split(',');
  const rounds = Number(option('rounds', 5));
  const warmup = Number(option('warmup', 1));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-bench-'));
  const classpath = (await minecraftLive.ensurePaper()).map((f) => path.join(minecraftLive.paperDir, f));
  const binaries = path.join(minecraftLive.paperDir, 'engine');
  const engines = specs.map((spec) => ({ spec, label: labelOf(spec), engine: makeEngine(spec) }));
  await Promise.all(engines.map(({ engine, label }) => engine.warm({ classpath, binaries, folder: path.join(scratch, `engine-${label}`), minecraftVersion: minecraftLive.minecraftVersion })));
  const verifier = new MinecraftVerifier({ root: path.join(scratch, 'verifier'), placementMode: 'updates', seed: process.argv.includes('--seeded') ? 1 : null });

  if (option('fuzz', null) !== null) return fuzz(engines, scratch);
  const items = loadItems();
  const results = [];
  for (const item of items) {
    const loaded = item.load();
    const folder = path.join(scratch, 'items', item.name);
    fs.mkdirSync(folder, { recursive: true });
    const commandsFile = path.join(folder, 'commands.txt');
    fs.writeFileSync(commandsFile, `${importCommands(loaded.world, null, Y_OFFSET).commands.join('\n')}\n`);
    const request = verifier.request(loaded.world, experiment(loaded.blocks, loaded.first, Number(option('verify-ticks', 100))));
    const requestFile = path.join(folder, 'request.json');
    fs.writeFileSync(requestFile, JSON.stringify({ ...verifier.transport(request), ...(process.argv.includes('--trace') ? { entity_trace: true } : {}) }));
    const prepared = { commandsFile, requestFile, blocks: loaded.blocks };
    const record = { name: item.name, kind: item.kind, blocks: loaded.world.size(), engines: {} };
    for (const { label } of engines) {
      record.engines[label] = { rounds: [] };
      fs.mkdirSync(path.join(folder, label), { recursive: true });
    }
    for (let round = 0; round < warmup + rounds; round++) {
      const order = engines.map((_, i) => engines[(i + round) % engines.length]);
      for (const { label, engine } of order) {
        const run = await runItem(engine, item, prepared, path.join(folder, label));
        if (round >= warmup) record.engines[label].rounds.push(run);
      }
    }
    // Medians and agreement.
    const summary = [];
    for (const { label } of engines) {
      const mine = record.engines[label];
      const failed = mine.rounds.find((run) => run.failed);
      if (failed) { mine.failed = failed.failed; continue; }
      mine.hashes = mine.rounds[0].hashes;
      mine.verifyFailed = mine.rounds.find((run) => run.verifyFailed)?.verifyFailed;
      mine.median = {};
      for (const phase of PHASES) {
        const values = mine.rounds.map((run) => run.times[phase]).filter((value) => value !== undefined);
        if (values.length) mine.median[phase] = median(values);
      }
      for (const run of mine.rounds) if (JSON.stringify(run.hashes) !== JSON.stringify(mine.hashes)) mine.unstable = true;
    }
    const base = engines[0].label;
    for (const { label } of engines.slice(1)) {
      const [a, b] = [record.engines[base], record.engines[label]];
      if (!a.hashes || !b.hashes) continue;
      const differs = Object.keys(a.hashes).filter((phase) => a.hashes[phase] !== b.hashes[phase]);
      if (differs.length) b.differs = differs;
    }
    for (const { label } of engines) {
      const mine = record.engines[label];
      summary.push(`${label} ${mine.failed ? `FAILED ${mine.failed.slice(0, 60)}` : PHASES.map((phase) => `${phase[0]}${mine.median[phase] === undefined ? '-' : mine.median[phase].toFixed(0)}`).join(' ')}${mine.verifyFailed ? ` (verify: ${mine.verifyFailed.slice(0, 50)})` : ''}${mine.differs ? ` DIFFERS ${mine.differs}` : ''}${mine.unstable ? ' UNSTABLE' : ''}`);
    }
    console.log(`${item.name} (${record.blocks} blocks): ${summary.join(' | ')}`);
    results.push(record);
  }
  // --graceful ends the engines by closing their input, which lets a program
  // built to record a profile (core-pgo) write it.
  if (process.argv.includes('--graceful')) await Promise.all(engines.map(({ engine }) => new Promise((resolve) => { engine.child.once('close', resolve); engine.child.stdin.end(); })));
  else await Promise.all(engines.map(({ engine }) => engine.stop()));
  fs.rmSync(scratch, { recursive: true, force: true });
  const output = { when: new Date().toISOString(), host: os.hostname(), specs, rounds, warmup, ticks: Number(option('ticks', 1000)), chunk: Number(option('chunk', 100)), phases: PHASES, results };
  if (option('json', null)) fs.writeFileSync(option('json'), JSON.stringify(output));
  report(output);
}

// The fuzzer's worlds (scripts/core-parity.js) run on every engine, untimed:
// to give an engine built to record a profile (scripts/core-pgo.js) the blocks
// and entities the corpus does not have. A refusal ends a case.
async function fuzz(engines, scratch) {
  const { FAMILIES, fuzzCase, registry } = require('./core-parity');
  const blocks = registry();
  const families = option('families', Object.keys(FAMILIES).join(',')).split(',');
  const seed = Number(option('seed', 1));
  const drops = process.argv.includes('--drops');
  const flow = process.argv.includes('--flow');
  const shift = process.argv.includes('--shift');
  let ran = 0;
  for (let i = 0; i < Number(option('fuzz')); i++) {
    const { steps } = fuzzCase(blocks, families, seed + i, Number(option('ticks', 40)), drops, flow, shift);
    for (const { engine, label } of engines) {
      const folder = path.join(scratch, `fuzz-${label}`);
      fs.mkdirSync(folder, { recursive: true });
      for (const [index, step] of steps.entries()) {
        const file = (name) => path.join(folder, `${index}-${name}`);
        try {
          if (step.place) {
            fs.writeFileSync(file('commands.txt'), `${step.place.join('\n')}\n`);
            await engine.place(file('commands.txt'), file('blueprint.bin'), { fresh: step.fresh });
          } else if (step.tick) await engine.tick(step.tick, file('patch.bin'), file('entities.json'));
          else await engine.input(...step.input);
        } catch (error) {
          if (/unported:/.test(error.message)) {
            countRefusal(error.message.replace(/^The engine failed: /, ''));
            break;
          }
        }
      }
      fs.rmSync(folder, { recursive: true, force: true });
    }
    ran++;
  }
  console.log(`${ran} fuzz cases`);
  printRefusals();
  if (process.argv.includes('--graceful')) await Promise.all(engines.map(({ engine }) => new Promise((resolve) => { engine.child.once('close', resolve); engine.child.stdin.end(); })));
  else await Promise.all(engines.map(({ engine }) => engine.stop()));
  fs.rmSync(scratch, { recursive: true, force: true });
}

// A table of the medians and totals of one run.
function report(output) {
  const labels = output.specs.map(labelOf);
  const fmt = (value) => (value === undefined ? '-' : value >= 100 ? value.toFixed(0) : value.toFixed(1));
  for (const kind of ['community', 'world']) {
    const items = output.results.filter((item) => item.kind === kind);
    if (!items.length) continue;
    // Totals over the items every engine ran fully.
    const ok = items.filter((item) => labels.every((label) => item.engines[label].median));
    console.log(`\n${kind}: ${items.length} items, ${ok.length} timed on every engine`);
    const header = ['phase', ...labels.map((l) => `${l} total ms`), ...labels.slice(1).map((l) => `${labels[0]}/${l}`)];
    console.log(header.join('\t'));
    for (const phase of output.phases) {
      const withPhase = ok.filter((item) => labels.every((label) => item.engines[label].median[phase] !== undefined) && !labels.some((label) => item.engines[label].verifyFailed && phase === 'verify'));
      const sums = labels.map((label) => withPhase.reduce((n, item) => n + item.engines[label].median[phase], 0));
      console.log([`${phase} (${withPhase.length})`, ...sums.map(fmt), ...sums.slice(1).map((s) => (sums[0] / s).toFixed(2))].join('\t'));
    }
    for (const item of items) for (const label of labels) { countRefusal(item.engines[label].failed); countRefusal(item.engines[label].verifyFailed); }
    const problems = items.filter((item) => labels.some((label) => item.engines[label].failed || item.engines[label].differs || item.engines[label].unstable || item.engines[label].verifyFailed));
    for (const item of problems) console.log(`  note ${item.name}: ${labels.map((label) => { const e = item.engines[label]; return e.failed ? `${label} failed ${e.failed.slice(0, 80)}` : [e.differs && `${label} differs ${e.differs}`, e.unstable && `${label} unstable`, e.verifyFailed && `${label} verify ${e.verifyFailed.slice(0, 80)}`].filter(Boolean).join(' '); }).filter(Boolean).join('; ')}`);
  }
  printRefusals();
}

function diff() {
  const [a, b] = [process.argv[process.argv.indexOf('--diff') + 1], process.argv[process.argv.indexOf('--diff') + 2]].map((file) => JSON.parse(fs.readFileSync(file, 'utf8')));
  const rows = [];
  for (const itemB of b.results) {
    const itemA = a.results.find((item) => item.name === itemB.name && item.kind === itemB.kind);
    if (itemA) rows.push([itemA, itemB]);
  }
  for (const label of labelsOf(a, b)) {
    for (const phase of b.phases) {
      let [x, y, n] = [0, 0, 0];
      for (const [ia, ib] of rows) {
        const [ma, mb] = [ia.engines[label]?.median?.[phase], ib.engines[label]?.median?.[phase]];
        if (ma === undefined || mb === undefined) continue;
        x += ma; y += mb; n++;
      }
      console.log(`${label} ${phase} (${n} items): ${x.toFixed(0)} ms -> ${y.toFixed(0)} ms (${(x / y).toFixed(2)}x)`);
    }
  }
}
const labelsOf = (a, b) => b.specs.map(labelOf).filter((label) => a.specs.map(labelOf).includes(label));

if (require.main === module) {
  (process.argv.includes('--diff') ? Promise.resolve(diff()) : main()).catch((error) => {
    console.error(error);
    process.exit(1);
  });
}

module.exports = { report };
