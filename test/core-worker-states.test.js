'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

// The engine worker (web/core-worker.mjs) over a stand-in world whose ticks
// fail when told to: whether the world runs, and what a stopped one refuses.
const SOURCE = fs.readFileSync(path.join(__dirname, '..', 'web', 'core-worker.mjs'), 'utf8');
const SPENT = 'The engine failed: unported: an earlier request reached what this port does not model';

function worker() {
  const posted = [];
  // The world: `trap` is what its next tick fails with, spending it.
  const world = { refused: null, trap: null, placed: 0 };
  class CoreWorld {
    get refused() { return world.refused; }
    place(commands, { fresh }) {
      if (!fresh && world.refused) throw new Error(SPENT);
      if (fresh) world.refused = null;
      world.placed += 1;
      return new Uint8Array(1);
    }
    spawnPlayer() { return new Float64Array(28); }
    tick() {
      if (world.refused) throw new Error(SPENT);
      if (world.trap) {
        world.refused = world.trap;
        world.trap = null;
        throw new Error(`The engine failed: ${world.refused}`);
      }
      return { bytes: new Uint8Array(1), entities: '[]' };
    }
  }
  const self = { postMessage: (message) => posted.push(message), onmessage: null };
  const context = vm.createContext({
    self, CoreWorld, performance, setTimeout, clearTimeout,
    fetch: async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) }),
    WebAssembly: { compile: async () => ({}) },
  });
  vm.runInContext(SOURCE.replace(/^import .*$/m, ''), context);
  let id = 0;
  // A request, resolved with its answer (rejected with its error).
  const ask = async (request) => {
    id += 1;
    const mine = id;
    self.onmessage({ data: { ...request, id: mine } });
    for (;;) {
      const answer = posted.find((message) => message.id === mine);
      if (answer?.error !== undefined) throw new Error(answer.error);
      if (answer) return answer;
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
  };
  const of = (type) => posted.filter((message) => message.type === type);
  const until = async (check, what) => {
    for (let i = 0; i < 400; i += 1) {
      if (check()) return;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    throw new Error(`Timed out waiting for ${what}`);
  };
  return { ask, of, until, world };
}

test('a world a failed tick stopped refuses to play or step, and says why once, until it is placed afresh', async () => {
  const w = worker();
  await w.ask({ type: 'load', wasm: 'http://app/core/a.wasm' });
  await w.ask({ type: 'place', commands: '', fresh: true });
  await w.ask({ type: 'run', playing: true });
  await w.until(() => w.of('frame').length >= 2, 'the world running');
  w.world.trap = 'unported: a block';
  await w.until(() => w.of('failed').length === 1, 'the tick that fails');
  assert.equal(w.of('failed')[0].error, 'The engine failed: unported: a block');
  const frames = w.of('frame').length;
  await assert.rejects(w.ask({ type: 'run', playing: true }), /cannot go from stopped to playing/);
  await assert.rejects(w.ask({ type: 'step' }), /cannot step while stopped/);
  // Told to pause, it stays stopped.
  await w.ask({ type: 'run', playing: false });
  await assert.rejects(w.ask({ type: 'run', playing: true }), /cannot go from stopped to playing/);
  await new Promise((resolve) => setTimeout(resolve, 120));
  assert.equal(w.of('failed').length, 1, 'the first reason is the only one told');
  assert.equal(w.of('frame').length, frames, 'no tick ran');
  // A fresh placement starts it over, paused: it steps and plays again.
  await w.ask({ type: 'place', commands: '', fresh: true });
  assert.equal((await w.ask({ type: 'step' })).ticks, 1);
  await w.ask({ type: 'run', playing: true });
  await w.until(() => w.of('frame').length >= frames + 3, 'the world running again');
  await w.ask({ type: 'run', playing: false });
  assert.equal(w.of('failed').length, 1);
});

test('a step whose tick fails stops the world, and a playing world told to play again goes on', async () => {
  const w = worker();
  await w.ask({ type: 'load', wasm: 'http://app/core/a.wasm' });
  await w.ask({ type: 'place', commands: '', fresh: true });
  await w.ask({ type: 'run', playing: true });
  await w.ask({ type: 'run', playing: true });
  await assert.rejects(w.ask({ type: 'step' }), /Pause the live world/);
  await w.ask({ type: 'run', playing: false });
  await w.ask({ type: 'run', playing: false });
  w.world.trap = 'unported: a block';
  await assert.rejects(w.ask({ type: 'step' }), /unported: a block/);
  assert.equal(w.of('failed').length, 1);
  await assert.rejects(w.ask({ type: 'step' }), /cannot step while stopped/);
  assert.equal(w.of('failed').length, 1);
});

// What the page's own count of the player rests on (live-world.mjs): requests
// are handled in the order they were asked, and a fresh placement takes the player.
test('a fresh placement asked after a spawn takes the player: input for it is refused until the next spawn', async () => {
  const w = worker();
  await w.ask({ type: 'load', wasm: 'http://app/core/a.wasm' });
  await w.ask({ type: 'place', commands: '', fresh: true });
  const input = { keys: 1, taps: 0, yaw: 0, pitch: 0 };
  // Asked together, as a spawn racing a restart is: answered in that order.
  const spawned = w.ask({ type: 'player', spawn: { x: 0, y: 64, z: 0, yaw: 0, pitch: 0, flying: true } });
  const placed = w.ask({ type: 'place', commands: '', fresh: true });
  const held = w.ask({ type: 'player', input });
  await spawned;
  await placed;
  await assert.rejects(held, /There is no player in the world/);
  // A placement on the world as it is keeps the player.
  await w.ask({ type: 'player', spawn: { x: 0, y: 64, z: 0, yaw: 0, pitch: 0, flying: true } });
  await w.ask({ type: 'place', commands: '', fresh: false });
  await w.ask({ type: 'player', input });
});
