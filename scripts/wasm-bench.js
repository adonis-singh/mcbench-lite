'use strict';

// How fast the WebAssembly build of the Rust engine ticks the live world, in
// Node and in Chrome (a Web Worker, through web/core-engine.mjs), beside the
// native program. Each build is placed as the live import places it, its
// levers are toggled once, and it is ticked the way the live world ticks:
// one tick per request (a patch and the entities each time), then the same
// ticks as one request.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/wasm-bench.js
//     [--builds name,...]      community builds, or a conversation id of the data folder
//     [--ticks N] [--rounds N] ticks per round (default 200), rounds kept best of (default 3)
//     [--jmath intel|fdlibm]   the module's Math.sin/cos flavour (default intel)
//     [--no-node] [--no-chrome] [--no-native]
//
// Chrome: see scripts/wasm-chrome.js.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { nativeEngine } = require('./wasm-native');
const { build: buildWasm } = require('./build-wasm');
const { communityCases, option } = require('./core-parity');
const { openPage } = require('./wasm-chrome');

const ROOT = path.join(__dirname, '..');
const BUILDS = option('builds', 'soontech-LC03,SQRT007,soontech-DC02-all,observer-clocks-10k,ROM001-programmed-1024,18611267-38d2-48e1-ae77-bff48124bf22').split(',');
const TICKS = Number(option('ticks', 200));
const ROUNDS = Number(option('rounds', 3));

// A build's commands as the live import places them, and the inputs that
// start it (each lever toggled once).
function workload(name) {
  // 10,000 observer clocks (two observers facing each other, a lamp behind
  // each): a world at work on every tick.
  if (name === 'observer-clocks-10k') {
    const commands = [];
    for (let i = 0; i < 100; i++) {
      for (let j = 0; j < 100; j++) {
        const [x, y, z] = [i * 5, 64, j * 2];
        commands.push(`setblock ${x} ${y} ${z} minecraft:redstone_lamp`, `setblock ${x + 3} ${y} ${z} minecraft:redstone_lamp`,
          `setblock ${x + 1} ${y} ${z} minecraft:observer[facing=east]`, `setblock ${x + 2} ${y} ${z} minecraft:observer[facing=west]`);
      }
    }
    return { name, commands: `${commands.join('\n')}\n`, inputs: [] };
  }
  if (/^[0-9a-f]{8}-/.test(name)) {
    const store = require('../server/store');
    const { importCommands } = require('../server/live-placement');
    const { Y_OFFSET } = require('../server/minecraft-dimension');
    const world = store.getConversation(name).world;
    return { name, commands: `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`, inputs: [] };
  }
  const { steps } = communityCases(2).find((entry) => entry.name === name).load();
  const inputs = steps.filter((step) => step.input).map((step) => step.input);
  return { name, commands: `${steps[0].place.join('\n')}\n`, inputs: inputs.filter((input, i) => inputs.findIndex((other) => other.join() === input.join()) === i) };
}

const round = (x) => Math.round(x * 10) / 10;

// The measurements one host makes of a workload, given the engine's calls.
// Every round places the build afresh and ticks it through the same ticks, so
// the rounds are the same work, and the best round is kept: the machine has
// other work, which only ever slows a round.
async function measure(load, { place, tick, tickMany, input, memory, now, cpu = null }) {
  const result = { name: load.name, commands: load.commands.split('\n').length - 1 };
  // Places the build, toggles its levers and ticks it through its first moves,
  // so the measurements are of a world at work (idle builds stay idle).
  const start = async () => {
    const t = now();
    const blueprint = await place(load.commands);
    const placeMs = now() - t;
    for (const args of load.inputs) await input(...args);
    await tickMany(20);
    return { placeMs, blueprint };
  };
  let best = null;
  for (let r = 0; r < ROUNDS; r++) {
    const round_ = {};
    const started = await start();
    if (r === 0) {
      result.placeMs = round(started.placeMs);
      result.blueprintMB = round(started.blueprint / 1048576);
    }
    const per = [];
    let active = 0;
    const cpuStart = cpu?.();
    for (let i = 0; i < TICKS; i++) {
      const t = now();
      const ticked = await tick();
      per.push(now() - t);
      // A tick with a patch bigger than an empty one's 36 bytes, or an entity.
      if (ticked && (ticked.bytes.length > 36 || ticked.entities.length)) active++;
    }
    round_.activePct = Math.round((100 * active) / TICKS);
    // Ticks per second of CPU time (Node only).
    if (cpu) round_.cpuTps = round((TICKS * 1000) / (cpu() - cpuStart));
    per.sort((a, b) => a - b);
    round_.perTickTps = round(1000 / (per.reduce((a, b) => a + b, 0) / per.length));
    round_.p50Ms = round(per[Math.floor(per.length / 2)]);
    round_.p99Ms = round(per[Math.floor(per.length * 0.99)]);
    round_.maxMs = round(per[per.length - 1]);
    await start();
    const t = now();
    await tickMany(TICKS);
    round_.batchTps = round((TICKS * 1000) / (now() - t));
    if (!best || round_.perTickTps > best.perTickTps) best = round_;
  }
  Object.assign(result, best);
  result.memoryMB = memory ? round(memory() / 1048576) : null;
  return result;
}

async function nodeRun(loads, module) {
  const { CoreWorld } = await import('../web/core-world.mjs');
  const results = [];
  for (const load of loads) {
    const t = performance.now();
    const world = new CoreWorld(module);
    const initMs = round(performance.now() - t);
    const result = await measure(load, {
      place: (commands) => world.place(commands).length,
      tick: () => { const r = world.tick(1); return { bytes: r.bytes, entities: JSON.parse(r.entities) }; },
      tickMany: (n) => world.tick(n),
      input: (type, x, y, z, yaw) => world.input(type, x, y, z, yaw),
      memory: () => world.exports.memory.buffer.byteLength,
      now: () => performance.now(),
      cpu: () => { const usage = process.cpuUsage(); return (usage.user + usage.system) / 1000; },
    });
    results.push({ ...result, initMs });
  }
  return results;
}

async function nativeRun(loads, jmath) {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-wasm-bench-'));
  const engine = nativeEngine(jmath);
  await engine.warm({ classpath: [], binaries: scratch, folder: path.join(scratch, 'engine'), minecraftVersion: '1.21.10' });
  const file = (name) => path.join(scratch, name);
  const results = [];
  for (const load of loads) {
    fs.writeFileSync(file('commands.txt'), load.commands);
    results.push(await measure(load, {
      place: async (commands) => { await engine.place(file('commands.txt'), file('blueprint.bin')); return fs.statSync(file('blueprint.bin')).size; },
      tick: async () => {
        await engine.tick(1, file('patch.bin'), file('entities.json'));
        return { bytes: fs.readFileSync(file('patch.bin')), entities: JSON.parse(fs.readFileSync(file('entities.json'), 'utf8')) };
      },
      tickMany: (n) => engine.tick(n, file('patch.bin'), file('entities.json')),
      input: (type, x, y, z, yaw) => engine.input(type, x, y, z, yaw),
      memory: null,
      now: () => performance.now(),
    }));
  }
  await engine.stop();
  fs.rmSync(scratch, { recursive: true, force: true });
  return results;
}

// The page Chrome runs: the same measurements through a worker.
const PAGE = `
import { CoreEngine } from '/web/core-engine.mjs';
window.run = async (loads, ticks, rounds) => {
  const out = [];
  for (const load of loads) {
    const commands = await (await fetch('/load/' + load.name + '.txt')).text();
    let t = performance.now();
    const engine = await CoreEngine.start('/core.wasm');
    const startMs = performance.now() - t;
    const r = (x) => Math.round(x * 10) / 10;
    const result = { name: load.name, commands: commands.split('\\n').length - 1, initMs: r(startMs) };
    const start = async () => {
      const t = performance.now();
      const blueprint = await engine.place(commands);
      const placeMs = performance.now() - t;
      for (const input of load.inputs) await engine.input(...input);
      await engine.tick(20);
      return { placeMs, blueprint };
    };
    let best = null;
    for (let k = 0; k < rounds; k++) {
      const started = await start();
      if (k === 0) {
        result.placeMs = r(started.placeMs);
        result.blueprintMB = r(started.blueprint.length / 1048576);
      }
      const per = [];
      let active = 0;
      for (let i = 0; i < ticks; i++) {
        const t = performance.now();
        const ticked = await engine.tick(1);
        per.push(performance.now() - t);
        if (ticked.bytes.length > 36 || ticked.entities.length) active++;
      }
      const round_ = { activePct: Math.round(100 * active / ticks) };
      per.sort((a, b) => a - b);
      round_.perTickTps = r(1000 / (per.reduce((a, b) => a + b, 0) / per.length));
      round_.p50Ms = r(per[Math.floor(per.length / 2)]);
      round_.p99Ms = r(per[Math.floor(per.length * 0.99)]);
      round_.maxMs = r(per[per.length - 1]);
      await start();
      const t = performance.now();
      await engine.tick(ticks);
      round_.batchTps = r(ticks * 1000 / (performance.now() - t));
      if (!best || round_.perTickTps > best.perTickTps) best = round_;
    }
    Object.assign(result, best);
    result.memoryMB = null;
    engine.terminate();
    out.push(result);
  }
  return out;
};
`;

async function chromeRun(loads, wasmFile) {
  const files = {};
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-wasm-bench-loads-'));
  for (const load of loads) {
    fs.writeFileSync(path.join(folder, `${load.name}.txt`), load.commands);
    files[`/load/${load.name}.txt`] = [path.join(folder, `${load.name}.txt`), 'text/plain'];
  }
  const { page, close } = await openPage(wasmFile, PAGE, files);
  try {
    return await page.evaluate(([list, ticks, rounds]) => window.run(list, ticks, rounds), [loads.map(({ name, inputs }) => ({ name, inputs })), TICKS, ROUNDS]);
  } finally {
    await close();
    fs.rmSync(folder, { recursive: true, force: true });
  }
}

async function main() {
  const jmath = option('jmath', 'intel');
  const wasmFile = await buildWasm({ jmath });
  const module = await WebAssembly.compile(fs.readFileSync(wasmFile));
  const loads = BUILDS.map(workload);
  console.log(`module ${fs.statSync(wasmFile).size} bytes, ${TICKS} ticks per measurement`);
  const report = {};
  if (!process.argv.includes('--no-node')) report.node = await nodeRun(loads, module);
  if (!process.argv.includes('--no-chrome')) report.chrome = await chromeRun(loads, wasmFile);
  if (!process.argv.includes('--no-native')) report.native = await nativeRun(loads, jmath);
  for (const [host, results] of Object.entries(report)) {
    console.log(`\n${host}`);
    console.table(results.map((r) => ({ build: r.name.slice(0, 24), commands: r.commands, 'init ms': r.initMs, 'place ms': r.placeMs, 'blueprint MB': r.blueprintMB, 'ticks/s (1 per request)': r.perTickTps, 'ticks/CPU s': r.cpuTps, 'p50 ms': r.p50Ms, 'p99 ms': r.p99Ms, 'max ms': r.maxMs, 'ticks/s (one request)': r.batchTps, 'active %': r.activePct, 'memory MB': r.memoryMB })));
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
