'use strict';

// Checks the WebAssembly build of the Rust engine against the native Rust
// program, byte for byte: both get the same requests and every reply and
// every byte they return must be identical (blueprints, patches, entity
// lists, and after every request the state dump: the random, scheduled
// ticks, block events, block entities, tickers), and a refusal ("unported:
// ...") must come at the same request with the same words. The wasm module
// runs in Node through web/core-world.mjs, the code the browser's worker
// runs.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/wasm-parity.js
//     [--jmath intel|fdlibm]                    Math.sin/cos flavour of both builds (default intel)
//     [--match regex] [--limit N] [--ticks N]   community builds (MCBENCH_CORPUS)
//     [--fuzz N] [--seed S] [--families a,b] [--shift] [--drops] [--flow] [--batch N]  random worlds
//     [--steps file] [--keep folder]            a kept case's steps.json; where mismatches are kept
//     [--player]                                the creative player's cases instead (playerCases): a player
//                                               walking through a piston door, and random walks
//     [--use]                                   right clicks (the "use" input, with the player's yaw) on every block
//                                               that takes one (doors, trapdoors, fence gates, repeaters, comparators,
//                                               note blocks, daylight detectors, redstone dust, decorated pots), among
//                                               redstone and sculk sensors (useCases), instead
//     [--orbs N] [--seed S]                     furnaces whose experience orbs TNT frees, and golem patterns
//                                               (scripts/orb-cases.js), instead
//     [--chrome]                                run the module in a Web Worker in Chrome (scripts/wasm-chrome.js),
//                                               comparing SHA-256 digests of the bytes instead of the bytes
//
// Cases are scripts/core-parity.js's: community builds placed as the live
// import places them and ticked one tick at a time with levers toggled and
// buttons pressed, or the fuzzer's worlds. The native program is built with
// the same jmath feature as the module, so the two differ in nothing but
// the target.

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { build: buildWasm } = require('./build-wasm');
const { FAMILIES, fuzzCase, registry, communityCases, useCases, run, option } = require('./core-parity');
const { openPage } = require('./wasm-chrome');
const { startNodeEngine } = require('./wasm-node-engine');
const { nativeEngine } = require('./wasm-native');

const ROOT = path.join(__dirname, '..');

// A case's steps on the engine worker (web/core-engine.mjs): each step's
// outcome and the bytes it returned, shaped as scripts/core-parity.js's run
// shapes the native program's, `encode` turning a Uint8Array into what is
// compared (the bytes, or in a page their digest). It runs in Node and, as the
// page's own code (its source is the page's), in Chrome: it names nothing
// outside itself.
async function runSteps(engine, steps, encode) {
  const outcomes = [];
  const text = (string) => new TextEncoder().encode(string);
  for (const step of steps) {
    let outcome;
    try {
      if (step.place) {
        outcome = { ok: 'placed', files: { blueprint: await encode(await engine.place(`${step.place.join('\n')}\n`, { fresh: step.fresh })) } };
      } else if (step.tick) {
        const ticked = await engine.request({ type: 'tick', ticks: step.tick });
        outcome = { ok: 'ticked', files: { patch: await encode(ticked.bytes), entities: await encode(text(ticked.entities)) } };
      } else if (step.spawn) {
        const [x, y, z, yaw, pitch, flying] = step.spawn;
        await engine.spawnPlayer({ x, y, z, yaw, pitch, flying });
        outcome = { ok: 'spawned', files: {} };
      } else if (step.player) {
        const [keys, yaw, pitch] = step.player;
        await engine.setPlayerInput({ keys, taps: 0, yaw, pitch });
        const ticked = await engine.request({ type: 'tick', ticks: 1 });
        outcome = { ok: 'ticked', files: { patch: await encode(ticked.bytes), entities: await encode(text(ticked.entities)), view: await encode(new Uint8Array(ticked.view.buffer)) } };
      } else {
        await engine.input(...step.input);
        outcome = { ok: 'input', files: {} };
      }
    } catch (error) {
      const reason = error.message.replace(/^The engine failed: /, '');
      outcomes.push({ failed: reason });
      if (reason.startsWith('unported:')) break;
      continue;
    }
    outcome.files.state = await encode(text((await engine.request({ type: 'state' })).text));
    outcomes.push(outcome);
  }
  return outcomes;
}

// The page Chrome runs: a case's steps through the worker, each file's bytes
// as the SHA-256 digest of them.
const PAGE = `
import { CoreEngine } from '/web/core-engine.mjs';
const engine = await CoreEngine.start('/core.wasm');
const hex = async (bytes) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map((b) => b.toString(16).padStart(2, '0')).join('');
${runSteps}
window.runCase = (steps) => runSteps(engine, steps, hex);
window.ready = true;
`;

// The creative player's cases: a build, a player, and each tick's keys and
// look as steps ({ spawn: [x, y, z, yaw, pitch, flying] }, { player: [keys, yaw, pitch] }).
function playerCases() {
  const cases = [];
  // A corridor with a piston door: two pistons in its wall hold their
  // heads across it, as long as redstone blocks power them. The blocks are
  // taken away and the pistons draw their heads back while the player walks
  // at the door, sprinting, then walks through, and on.
  const door = ['fill 0 63 0 4 63 30 minecraft:stone', 'fill 1 64 0 1 67 30 minecraft:stone', 'fill 3 64 0 3 67 30 minecraft:stone', 'fill 2 67 0 2 67 30 minecraft:stone',
    'fill 4 64 0 4 67 30 minecraft:stone',
    'setblock 3 64 10 minecraft:piston[facing=west]', 'setblock 3 65 10 minecraft:piston[facing=west]',
    'setblock 4 64 10 minecraft:redstone_block', 'setblock 4 65 10 minecraft:redstone_block'];
  const steps = [{ place: door, fresh: true }, { tick: 10 }, { spawn: [2.5, 64, 4.5, 0, 0, false] }];
  for (let t = 0; t < 12; t += 1) steps.push({ player: [t < 3 ? 0 : 1, 0, 0] });
  steps.push({ place: ['setblock 4 64 10 minecraft:air', 'setblock 4 65 10 minecraft:air'], fresh: false });
  for (let t = 0; t < 60; t += 1) steps.push({ player: [t < 20 ? 1 | 64 : 1, 0, 0] });
  // Back through it, jumping, looking around, then flying.
  for (let t = 0; t < 40; t += 1) steps.push({ player: [t % 10 === 0 ? 2 | 16 : 2, 180, 15] });
  steps.push({ place: ['setblock 4 64 10 minecraft:redstone_block', 'setblock 4 65 10 minecraft:redstone_block'], fresh: false });
  for (let t = 0; t < 40; t += 1) steps.push({ player: [t < 10 ? 1 : 1 | 16, 0, 0] });
  cases.push({ name: 'player-piston-door', steps });
  // Random walks over a room with stairs, slabs, water, ice, a ladder and a lever.
  for (let seed = 1; seed <= 6; seed += 1) {
    let state = seed * 7919;
    const random = (n) => { state = (state * 1103515245 + 12345) & 0x7fffffff; return state % n; };
    const room = ['fill 0 63 0 15 63 15 minecraft:stone', 'fill 0 64 0 15 70 0 minecraft:stone', 'fill 0 64 15 15 70 15 minecraft:stone', 'fill 0 64 0 0 70 15 minecraft:stone', 'fill 15 64 0 15 70 15 minecraft:stone',
      'fill 3 64 3 6 64 6 minecraft:oak_stairs[facing=east,half=bottom]', 'fill 8 64 3 11 64 6 minecraft:stone_slab[type=bottom]', 'fill 3 64 9 6 64 12 minecraft:water', 'fill 8 63 9 11 63 12 minecraft:ice',
      'setblock 14 64 14 minecraft:ladder[facing=west]', 'fill 14 65 14 14 68 14 minecraft:ladder[facing=west]', 'setblock 7 64 7 minecraft:lever[face=floor,facing=north]'];
    const walk = [{ place: room, fresh: true }, { tick: 3 }, { spawn: [2.5, 64, 2.5, random(360), 0, seed % 2 === 0] }];
    let keys = 0;
    let yaw = 0;
    for (let t = 0; t < 200; t += 1) {
      if (random(8) === 0) keys = random(128);
      if (random(5) === 0) yaw = random(360) - 180;
      walk.push({ player: [keys, yaw, random(120) - 60] });
      if (random(40) === 0) walk.push({ input: ['toggle', 7, 64, 7, 0] });
    }
    cases.push({ name: `player-walk-${seed}`, steps: walk });
  }
  return cases;
}

const digest = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const digests = (outcomes) => outcomes.map((o) => (o.files ? { ...o, files: Object.fromEntries(Object.entries(o.files).map(([name, bytes]) => [name, digest(bytes)])) } : o));
const same = (a, b) => (typeof a === 'string' ? a === b : a?.equals(b));

// Where the module's outcomes first differ from the native program's, or
// null. A refusal both make at the same request, with the same words, is no
// difference.
function compare(native, wasm, steps, labels = ['native', 'wasm']) {
  for (let i = 0; i < Math.max(native.length, wasm.length); i++) {
    const [n, w] = [native[i], wasm[i]];
    const what = steps[i].place ? 'place' : steps[i].tick ? 'tick' : steps[i].player ? 'player' : steps[i].spawn ? 'spawn' : 'input';
    if (!n || !w) return { step: i, what, difference: `${labels[0]} ${n ? n.ok || `failed: ${n.failed}` : 'stopped'}, ${labels[1]} ${w ? w.ok || `failed: ${w.failed}` : 'stopped'}` };
    if (n.failed !== w.failed || n.ok !== w.ok) return { step: i, what, difference: `${labels[0]} ${n.ok || `failed: ${n.failed.slice(0, 1500)}`}, ${labels[1]} ${w.ok || `failed: ${w.failed.slice(0, 1500)}`}` };
    if (n.failed) continue;
    for (const name of new Set([...Object.keys(n.files), ...Object.keys(w.files)])) {
      if (!same(n.files[name], w.files[name])) return { step: i, what, difference: `${name} differs (${labels[0]} ${n.files[name]?.length} bytes, ${labels[1]} ${w.files[name]?.length})`, file: name };
    }
  }
  return null;
}

let chromeDigest = false;

async function main() {
  const jmath = option('jmath', 'intel');
  const wasmFile = await buildWasm({ jmath });
  const native = nativeEngine(jmath);
  const chrome = process.argv.includes('--chrome') ? await openPage(wasmFile, PAGE) : null;
  if (chrome) await chrome.page.waitForFunction(() => window.ready);
  // The engine worker: in Chrome's page, or in a Node worker thread (the same worker code).
  const nodeEngine = chrome ? null : await startNodeEngine(wasmFile);
  chromeDigest = Boolean(chrome);
  const wasmSteps = (steps) => (chrome ? chrome.page.evaluate((list) => window.runCase(list), steps) : runSteps(nodeEngine, steps, (bytes) => Buffer.from(bytes)));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-wasm-parity-'));
  await native.warm({ classpath: [], binaries: path.join(scratch, 'binaries'), folder: path.join(scratch, 'native'), minecraftVersion: '1.21.10' });
  const out = path.join(scratch, 'native-out');
  fs.mkdirSync(out);
  const keep = option('keep', null);
  const ticks = Number(option('ticks', 100));
  let cases;
  if (option('steps', null) !== null) {
    cases = [{ load: () => ({ name: 'steps', steps: JSON.parse(fs.readFileSync(option('steps'), 'utf8')) }) }];
  } else if (process.argv.includes('--player')) {
    cases = playerCases().map(({ name, steps }) => ({ load: () => ({ name, steps }) }));
  } else if (process.argv.includes('--use')) {
    cases = useCases().map(({ name, steps }) => ({ load: () => ({ name, steps }) }));
  } else if (option('orbs', null) !== null) {
    cases = require('./orb-cases').orbCases(Number(option('orbs')), Number(option('seed', 1)), require('./core-parity').random).map(({ name, steps }) => ({ load: () => ({ name, steps }) }));
  } else if (option('fuzz', null) !== null) {
    const blocks = registry();
    const families = option('families', Object.keys(FAMILIES).join(',')).split(',');
    const seed = Number(option('seed', 1));
    cases = Array.from({ length: Number(option('fuzz')) }, (_, i) => ({ name: `fuzz-${seed + i}`, load: () => fuzzCase(blocks, families, seed + i, Number(option('ticks', 40)), process.argv.includes('--drops'), process.argv.includes('--flow'), process.argv.includes('--shift')) }));
  } else {
    cases = communityCases(ticks).filter(({ name }) => new RegExp(option('match', '')).test(name)).slice(0, Number(option('limit', Infinity)));
  }
  const totals = { matched: 0, refusedSame: 0, mismatched: 0, requests: 0, bytes: 0 };
  for (const entry of cases) {
    const { name, steps } = entry.load();
    const [nativeBytes, wasmOutcomes] = await Promise.all([run(native, out, steps), wasmSteps(steps)]);
    const nativeOutcomes = chrome ? digests(nativeBytes) : nativeBytes;
    const result = compare(nativeOutcomes, wasmOutcomes, steps);
    totals.requests += wasmOutcomes.length;
    for (const outcome of nativeBytes) for (const bytes of Object.values(outcome.files || {})) totals.bytes += bytes.length;
    if (result) {
      totals.mismatched++;
      console.log(`MISMATCH  ${name} at step ${result.step} (${result.what}): ${result.difference}`);
      if (keep) {
        fs.mkdirSync(path.join(keep, name), { recursive: true });
        fs.writeFileSync(path.join(keep, name, 'steps.json'), JSON.stringify(steps, null, 1));
        for (const [side, outcomes] of [['native', nativeOutcomes], ['wasm', wasmOutcomes]]) {
          for (const [file, bytes] of Object.entries(outcomes[result.step]?.files || {})) fs.writeFileSync(path.join(keep, name, `${side}-${result.step}-${file}`), bytes);
        }
      }
    } else if (wasmOutcomes.at(-1)?.failed?.startsWith('unported:')) {
      totals.refusedSame++;
      console.log(`refused   ${name} at step ${wasmOutcomes.length - 1}, both: ${wasmOutcomes.at(-1).failed}`);
    } else {
      totals.matched++;
      console.log(`match     ${name} (${steps.length} steps)`);
    }
  }
  console.log(JSON.stringify(totals));
  await native.stop();
  await chrome?.close();
  nodeEngine?.terminate();
  fs.rmSync(scratch, { recursive: true, force: true });
  // A refusal by one side only is a mismatch (compare). A case that throws
  // ends the run, so none is counted as errored.
  console.log(`${totals.matched} matched, ${totals.mismatched} differed, ${totals.refusedSame} refused by both, 0 errored`);
  // A run that compared nothing proves nothing.
  if (totals.mismatched || !totals.matched) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
