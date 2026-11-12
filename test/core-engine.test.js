'use strict';

const test = require('node:test');
const assert = require('node:assert');
const { build } = require('../scripts/build-wasm');
const { startNodeEngine } = require('../scripts/wasm-node-engine');

// The engine in its worker (web/core-worker.mjs): requests in order, frames
// of the running world, and refusals.
const COMMANDS = [
  'fill 0 64 0 5 64 5 minecraft:stone',
  'setblock 1 65 1 minecraft:lever[face=floor,facing=north,powered=false]',
  'setblock 2 65 1 minecraft:redstone_wire',
  'setblock 3 65 1 minecraft:redstone_lamp',
].map((line) => `${line}\n`).join('');

let wasm;
test.before(async () => { wasm = await build(); });

async function engine(t) {
  const started = await startNodeEngine(wasm);
  t.after(() => started.terminate());
  return started;
}

const nextFrames = (started, count) => new Promise((resolve) => {
  const frames = [];
  started.onframe = (frame) => { frames.push(frame); if (frames.length === count) resolve(frames); };
});

test('a running world ticks on the clock, a frame each, and stops when told', async (t) => {
  const started = await engine(t);
  await started.place(COMMANDS);
  const frames = nextFrames(started, 6);
  const startedAt = performance.now();
  await started.run(true);
  const run = await frames;
  const elapsed = performance.now() - startedAt;
  assert.deepStrictEqual(run.map((frame) => frame.ticks), [1, 2, 3, 4, 5, 6]);
  assert.ok(elapsed > 200 && elapsed < 450, `six ticks took ${elapsed} ms`);
  assert.ok(run[0].bytes instanceof Uint8Array && run[0].bytes.length > 0, 'a frame carries its patch');
  assert.strictEqual(run[0].entities, '[]');
  const ticks = await started.run(false);
  const seen = [];
  started.onframe = (frame) => seen.push(frame);
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.deepStrictEqual(seen, [], 'a world that was stopped ticks no more');
  assert.ok(ticks >= 6);
  // One tick while stopped.
  const stepped = nextFrames(started, 1);
  assert.strictEqual(await started.step(), ticks + 1);
  assert.strictEqual((await stepped)[0].ticks, ticks + 1);
});

test('a fresh placement starts the count again, and an edit does not', async (t) => {
  const started = await engine(t);
  await started.place(COMMANDS);
  await started.tick(3);
  const frames = nextFrames(started, 1);
  await started.place('setblock 4 65 1 minecraft:redstone_lamp\n', { fresh: false });
  await started.run(true);
  assert.strictEqual((await frames)[0].ticks, 1, 'ticks counted since the fresh placement: manual ticks are not frames');
  await started.run(false);
  await started.place(COMMANDS);
  const again = nextFrames(started, 1);
  await started.run(true);
  assert.strictEqual((await again)[0].ticks, 1);
});

test('a refusal fails the request and spends the world until a fresh placement', async (t) => {
  const started = await engine(t);
  await started.place(COMMANDS);
  await started.tick(1);
  await started.input('toggle', 1, 65, 1, 0);
  await assert.rejects(started.input('toggle', 0, 64, 0, 0), /No toggle input/); // a failure Minecraft reported
  await started.place('setblock 4 65 4 minecraft:infested_stone\nsetblock 5 65 4 minecraft:tnt[unstable=false]\nsetblock 5 65 3 minecraft:redstone_block\n', { fresh: false });
  // The TNT the redstone block primes explodes on the 80th tick, beside infested stone, which would spawn a silverfish.
  await assert.rejects(started.tick(80), /^Error: The engine failed: unported: InfestedBlock.spawnAfterBreak/);
  await assert.rejects(started.tick(1), /unported: an earlier request/);
  await started.place(COMMANDS);
  assert.ok((await started.tick(1)).bytes.length > 0);
});

test('a running world that is refused stops and says so', async (t) => {
  const started = await engine(t);
  await started.place(`${COMMANDS}setblock 4 65 4 minecraft:infested_stone\nsetblock 5 65 4 minecraft:tnt[unstable=false]\nsetblock 5 65 3 minecraft:redstone_block\n`);
  const failed = new Promise((resolve, reject) => {
    started.onfailed = resolve;
    // A world the engine does not refuse ticks on: a frame past the TNT's tick fails the test instead of a wait for nothing.
    started.onframe = (frame) => { if (frame.ticks > 80) reject(new Error(`no refusal by tick ${frame.ticks}`)); };
  });
  await started.run(true);
  assert.match(await failed, /^The engine failed: unported: InfestedBlock.spawnAfterBreak/);
  // It stopped: no frames come after.
  const seen = [];
  started.onframe = (frame) => seen.push(frame);
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.deepStrictEqual(seen, []);
});

test('a request that spends the world stops it and says so once, as does a fresh placement that fails', async (t) => {
  const started = await engine(t);
  const resin = 'setblock 0 65 0 minecraft:resin_clump[down=true]\n';
  await started.place(COMMANDS);
  const failures = [];
  started.onfailed = (message) => failures.push(message);
  await started.run(true);
  // A step asked of a playing world is refused without stopping it.
  await assert.rejects(started.step(), /Pause the live world/);
  assert.equal((await nextFrames(started, 1)).length, 1, 'still running');
  await assert.rejects(started.edit(resin), /^Error: The engine failed: unported: /);
  assert.equal(failures.length, 1);
  assert.match(failures[0], /^The engine failed: unported: /);
  const seen = [];
  started.onframe = (frame) => seen.push(frame);
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.deepEqual(seen, [], 'it stopped');
  // Requests on the spent world fail, and tell nothing again; stopped, it
  // refuses to step or play.
  await assert.rejects(started.input('toggle', 1, 65, 1, 0), /an earlier request/);
  await assert.rejects(started.step(), /cannot step while stopped/);
  await assert.rejects(started.run(true), /cannot go from stopped to playing/);
  await started.run(false);
  await started.removePlayer();
  await new Promise((resolve) => setTimeout(resolve, 120));
  assert.equal(failures.length, 1);
  await assert.rejects(started.place(`${COMMANDS}${resin}`), /unported/);
  assert.equal(failures.length, 2, 'a fresh placement paused the world, and its failure says so');
  await started.place(COMMANDS);
  assert.ok((await started.tick(1)).bytes.length > 0);
});

test('the world exports as a save', async (t) => {
  const started = await engine(t);
  await started.place(COMMANDS);
  await started.tick(5);
  const save = await started.export();
  assert.strictEqual(save.gameTime, 5n);
  assert.deepStrictEqual(save.chunks.map(({ x, z }) => [x, z]), [[0, 0]]);
  assert.ok(save.chunks[0].nbt.length > 1000);
});

test('a player in a running world is ticked with it, on the keys it holds, and its view comes with each frame', async (t) => {
  const started = await engine(t);
  await started.place('fill 0 63 0 20 63 20 minecraft:stone\n');
  const view = await started.spawnPlayer({ x: 2.5, y: 64, z: 2.5, yaw: 0, pitch: 0, flying: false });
  assert.strictEqual(view[2], 2.5);
  const frames = nextFrames(started, 6);
  await started.setPlayerInput({ keys: 1, taps: 0, yaw: 0, pitch: 0 });
  await started.run(true);
  const run = await frames;
  await started.run(false);
  assert.ok(run.every((frame) => frame.view.length === 28));
  assert.ok(run.at(-1).view[2] > run[0].view[2], 'the player walked forward');
  // One tick at a time with a player; no input without one.
  await assert.rejects(started.tick(2), /one tick at a time/);
  const manual = await started.tick(1);
  assert.strictEqual(manual.view.length, 28);
  await started.removePlayer();
  await assert.rejects(started.setPlayerInput({ keys: 1, taps: 0, yaw: 0, pitch: 0 }), /no player/);
  assert.strictEqual((await started.tick(2)).view, undefined);
});
