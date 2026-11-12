'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// The page's live world (web/live-world.mjs) against a stand-in server and
// engine whose answers the tests hold back and let go, for the orders of
// events the real ones make rare: a poll, a restart or a switch of view
// during a sync, a start that fails, a reply that comes late.
let LiveWorld;
let patchBytes;
let blueprint;
test.before(async () => {
  ({ LiveWorld } = await import('../web/live-world.mjs'));
  const { writePatch, emptyBlueprint } = await import('../web/live-patch.mjs');
  patchBytes = () => writePatch({ entries: [], bounds: [0, 0, 0, 0, 0, 0], columns: 1, changes: [] });
  blueprint = emptyBlueprint;
});

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}

// Gates: the next call of a kind waits for the deferred `gate` returns.
class Gates {
  constructor() { this.held = new Map(); this.waiting = new Set(); }
  gate(kind) { const d = deferred(); this.held.set(kind, d); return d; }
  async pass(kind) {
    const d = this.held.get(kind);
    if (!d) return;
    this.held.delete(kind);
    this.waiting.add(kind);
    try { await d.promise; } finally { this.waiting.delete(kind); }
  }
}

class FakeEngine extends Gates {
  constructor(address) {
    super();
    this.address = address;
    this.calls = [];
    this.dead = null;
    this.onframe = null;
    this.onfailed = null;
    this.refuse = null; // a failure each place and edit answers
    this.placed = []; // what each placement was asked with
    // The worker's player, as the requests leave it in the order they were asked.
    this.player = false;
  }
  async request(type, args, answer) {
    if (this.dead) throw this.dead;
    this.calls.push([type, ...args]);
    await this.pass(type);
    if (this.dead) throw this.dead;
    return answer();
  }
  place(commands, { fresh }) {
    this.placed.push({ commands, fresh });
    if (fresh) this.player = false;
    return this.request('place', [fresh], () => {
      if (this.refuse) throw new Error(this.refuse);
      return blueprint();
    });
  }
  edit() {
    return this.request('edit', [], () => {
      if (this.refuse) throw new Error(this.refuse);
      return { bytes: patchBytes(), entities: [] };
    });
  }
  run(playing) { return this.request('run', [playing], () => 0); }
  step() { return this.request('step', [], () => 0); }
  input() { return this.request('input', [], () => undefined); }
  spawnPlayer(spawn) {
    this.player = true;
    return this.request('spawn', [spawn.x], () => new Float64Array(28).fill(spawn.x));
  }
  setPlayerInput() {
    const { player } = this;
    return this.request('playerInput', [], () => { if (!player) throw new Error('The engine failed: There is no player in the world'); });
  }
  removePlayer() {
    this.player = false;
    return this.request('removePlayer', [], () => undefined);
  }
  terminate() { this.dead = new Error('The engine was stopped'); }
  frame(ticks) { this.onframe({ ticks, bytes: patchBytes(), entities: '[]', view: null }); }
  ran() { return this.calls.filter(([type]) => type === 'run').map(([, playing]) => playing); }
}

// What the import answers with: the whole world, or what takes an earlier version to the latest.
const WHOLE = 'setblock 0 64 0 minecraft:stone\n';
const DELTA = 'setblock 1 64 0 minecraft:stone\n';

function fakeHost({ sleep = (ms) => new Promise((resolve) => setTimeout(resolve, Math.min(ms, 5))) } = {}) {
  const h = new Gates();
  Object.assign(h, {
    server: { version: 1, core: 'core-a', mask: { minX: 0, maxX: 15, minZ: 0, maxZ: 15 }, blockCount: 1 },
    calls: [],
    engines: [],
    starts: 0,
    startFails: null,
    importFails: null,
    async json(path_) {
      const url = new URL(path_, 'http://app');
      const key = url.searchParams.get('view');
      h.calls.push(`${url.pathname.slice('/api/minecraft/'.length)} ${key}${url.searchParams.has('since') ? ` since ${url.searchParams.get('since')}` : ''}`);
      const kind = url.pathname.endsWith('status') ? 'status' : 'import';
      // A request takes a turn of the event loop, as one over the network does.
      await new Promise((resolve) => setImmediate(resolve));
      await h.pass(kind);
      const { version, core, mask, blockCount } = h.server;
      if (kind === 'status') {
        return { yOffset: 64, mesher: {}, core, coreError: null, view: key ? { version, mask, blockCount, bounds: { min: [0, 0, 0], max: [0, 0, 0] } } : null };
      }
      if (h.importFails) throw new Error(h.importFails);
      const since = url.searchParams.has('since') ? Number(url.searchParams.get('since')) : null;
      return { fresh: since === null, version, blockCount, commands: since === version ? '' : since === null ? WHOLE : DELTA };
    },
    async startEngine(address) {
      h.starts += 1;
      await h.pass('start');
      if (h.startFails) throw new Error(h.startFails);
      const engine = new FakeEngine(address);
      h.engines.push(engine);
      return engine;
    },
    blobUrl: (() => { let n = 0; return () => `blob:${n += 1}`; })(),
    revoke() {},
    sleep,
    imports() { return h.calls.filter((call) => call.startsWith('import')); },
  });
  return h;
}

async function until(check, what, ms = 4000) {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = check();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
}
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function open(t, options) {
  const h = fakeHost(options);
  const live = new LiveWorld(h);
  t.after(() => live.dispose());
  return { live, h };
}

// Starts looking, and resolves with the engine once `key` is placed and running.
async function running(live, h, key = 'chat') {
  live.begin();
  await live.show(key);
  await until(() => live.state.status === 'running' && live.state.conversationId === key && h.engines.at(-1)?.ran().at(-1) === true, 'the world running');
  return h.engines.at(-1);
}

test('an engine that stopped the world keeps its error through polls, and a restart plays the world again', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  engine.onfailed('The engine failed: unported: a block');
  await pause(40);
  assert.equal(live.state.status, 'error');
  assert.match(live.state.error, /^The live world stopped: The engine failed: unported: a block/);
  assert.equal(live.state.live.playing, false);
  await live.control('restart');
  await until(() => live.state.status === 'running' && live.state.live.playing, 'playing again');
  assert.deepEqual(engine.calls.filter(([type]) => type === 'place'), [['place', true], ['place', true]], 'placed afresh');
  assert.deepEqual(engine.ran(), [true, true]);
});

test('a world the engine stopped refuses to play or step, and keeps its first reason until it is placed afresh', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  engine.onfailed('The engine failed: unported: a block');
  await assert.rejects(live.control('play'), /cannot go from stopped to playing/);
  await assert.rejects(live.control('pause'), /cannot go from stopped to paused/);
  await assert.rejects(live.control('step'), /cannot step while stopped/);
  assert.equal(live.state.live.stopped, true, 'the page is told, for its controls');
  assert.deepEqual(engine.ran(), [true], 'the engine is not asked to run the stopped world');
  assert.ok(!engine.calls.some(([type]) => type === 'step'), 'nor to step it');
  // What a spent world fails with after it does not replace the reason.
  engine.onfailed('The engine failed: unported: an earlier request reached what this port does not model');
  assert.match(live.state.error, /^The live world stopped: The engine failed: unported: a block$/);
  assert.equal(live.state.live.playing, false);
  await live.control('restart');
  await until(() => live.state.status === 'running' && live.state.live.playing, 'playing again');
  assert.equal(live.state.error, null);
  assert.equal(live.state.live.stopped, false);
  await live.control('pause');
  await live.control('step');
  assert.deepEqual(engine.ran(), [true, true, false]);
});

test('a first sync that failed has nothing on show and says so, and a restart tries it again', async (t) => {
  const { live, h } = open(t);
  h.importFails = 'HTTP 503';
  live.begin();
  await live.show('chat');
  await until(() => live.state.status === 'error', 'the failed sync');
  assert.equal(live.state.error, 'HTTP 503');
  assert.equal(live.state.viewerRevision, null);
  assert.equal(live.state.live, null, 'no base: the live controls have nothing to draw from');
  const failed = live.state.syncFailed;
  const tries = h.imports().length;
  await pause(60);
  assert.equal(h.imports().length, tries, 'the polls do not try the version that failed again');
  h.importFails = null;
  await live.control('restart');
  await until(() => live.state.status === 'running' && h.engines.at(-1)?.ran().at(-1) === true, 'placed and running');
  assert.equal(h.imports().length, tries + 1, 'one more try, the restart\'s');
  // What the page draws its way out from, with no base for the live controls.
  assert.equal(failed, true, 'the page was told the sync failed');
  assert.equal(live.state.syncFailed, false);
});

test('a failed sync is not tried again until the server has another version or a restart is asked for', async (t) => {
  const { live, h } = open(t);
  live.begin();
  h.startFails = null;
  await live.show('chat');
  await until(() => h.engines.length === 1, 'the engine');
  await until(() => live.state.status === 'running', 'running');
  h.engines[0].refuse = 'The engine failed: unported: a block';
  h.server.version = 2;
  await until(() => live.state.status === 'error', 'the failure');
  const tries = h.imports().length;
  await pause(60);
  assert.equal(h.imports().length, tries, 'no sync again for the version that failed');
  assert.equal(live.phase, 'failed');
  h.engines[0].refuse = null;
  h.server.version = 3;
  await until(() => live.state.status === 'running' && live.shownVersion === 3, 'the next version');
  assert.equal(h.imports().length, tries + 1);
});

test('a failed engine start is tried again by a restart, not by every poll', async (t) => {
  const { live, h } = open(t);
  h.startFails = 'The module is unavailable';
  live.begin();
  await live.show('chat');
  await until(() => live.state.status === 'error', 'the failed start');
  assert.equal(live.state.error, 'The module is unavailable');
  await pause(60);
  assert.equal(h.starts, 1);
  h.startFails = null;
  await live.control('restart');
  await until(() => live.state.status === 'running', 'started');
  assert.equal(h.starts, 2);
});

test('another engine module ends the engine and places the view afresh in a new one', async (t) => {
  const { live, h } = open(t);
  const first = await running(live, h);
  h.server.core = 'core-b';
  await until(() => h.engines.length === 2 && live.state.status === 'running' && h.engines[1].ran().at(-1) === true, 'the new engine');
  assert.ok(first.dead, 'the old engine is ended');
  assert.equal(h.engines[1].address, 'core-b');
  assert.deepEqual(h.engines[1].calls[0], ['place', true]);
});

test('a restart during a sync runs after it', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  const held = h.gate('import');
  h.server.version = 2;
  await until(() => h.waiting.has('import'), 'the sync of version 2');
  await live.control('restart');
  held.resolve();
  await until(() => engine.calls.filter(([type, fresh]) => type === 'place' && fresh).length === 2, 'the restart');
  await until(() => live.state.status === 'running', 'running');
  assert.deepEqual(h.imports().slice(-2), ['import chat since 1', 'import chat'], 'the edit, then the restart from nothing');
  assert.equal(live.shownVersion, 2);
});

test('a switch of view takes the new view at once: no look in between syncs the old one', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h, 'a');
  const pausing = engine.gate('run');
  const before = h.calls.length;
  const switching = live.show('b');
  assert.equal(live.key, 'b');
  assert.equal(live.state.conversationId, 'b');
  assert.equal(live.state.viewerRevision, null);
  // A frame the engine sent before it heard of b reaches nothing.
  const frames = [];
  live.onFrame((frame) => frames.push(frame));
  engine.frame(9);
  assert.deepEqual(frames, []);
  await pause(30);
  assert.ok(h.calls.slice(before).every((call) => call.includes(' b')), h.calls.slice(before).join(', '));
  pausing.resolve();
  await switching;
});

test('a world let go while its engine starts ends that engine', async (t) => {
  const { live, h } = open(t);
  const start = h.gate('start');
  live.begin();
  await live.show('chat');
  await until(() => h.waiting.has('start'), 'the start');
  live.dispose();
  start.resolve();
  await until(() => h.engines.length === 1, 'started');
  await pause(5);
  assert.ok(h.engines[0].dead, 'ended once started');
});

test('a wake during a look looks again right after it', async (t) => {
  const { live, h } = open(t, { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)) });
  const held = h.gate('status');
  live.begin();
  await until(() => h.waiting.has('status'), 'the look');
  live.wake();
  held.resolve();
  await until(() => h.calls.filter((call) => call.startsWith('status')).length === 2, 'the second look', 300);
});

test('the last tick count is drawn after a burst of ticks, between polls', async (t) => {
  const { live, h } = open(t, { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)) });
  const engine = await running(live, h);
  // Past the looks of the sync: the next is a second away.
  await pause(120);
  for (let ticks = 1; ticks <= 5; ticks += 1) engine.frame(ticks);
  await until(() => live.state.live.ticks === 5, 'the last count', 400);
});

test('a base is drawn with the description of the version it shows, though the build changed during its sync', async (t) => {
  const { live, h } = open(t);
  live.begin();
  const held = h.gate('import');
  await live.show('chat');
  await until(() => h.waiting.has('import'), 'the first sync');
  h.server.version = 2;
  h.server.mask = { minX: 0, maxX: 31, minZ: 0, maxZ: 31 };
  await until(() => live.view.version === 2, 'a poll of version 2');
  held.resolve();
  await until(() => live.state.status === 'running' && live.shownVersion === 2, 'version 2');
  assert.equal(live.state.viewerMask.maxX, 31, 'version 2 is drawn with its own mask');
  assert.equal(h.engines[0].placed.length, 1, 'placed once');
});

test('an import that answers with a later version than the poll described is not placed: the page looks again', async (t) => {
  const { live, h } = open(t);
  live.begin();
  const held = h.gate('import');
  await live.show('chat');
  await until(() => h.waiting.has('import'), 'the first sync');
  // The build changes after the poll and before the import: the import is
  // the first to answer with version 2, and no poll has described it.
  const poll = h.gate('status');
  await until(() => h.waiting.has('status'), 'the next poll');
  h.server.version = 2;
  h.server.mask = { minX: 0, maxX: 31, minZ: 0, maxZ: 31 };
  held.resolve();
  await until(() => live.phase === 'waiting', 'the sync ended');
  assert.equal(live.shownVersion, null, 'version 2 is not on show with version 1\'s description');
  assert.equal(h.starts, 0, 'nothing was placed');
  poll.resolve();
  await until(() => live.state.status === 'running' && live.shownVersion === 2, 'version 2');
  assert.equal(live.state.viewerMask.maxX, 31, 'version 2 is drawn with its own mask');
  assert.deepEqual(h.imports(), ['import chat', 'import chat']);
  assert.equal(h.engines[0].placed.length, 1, 'placed once');
});

test('a delta asked for before the engine\'s worker failed is not placed as the whole world in the next engine', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  const held = h.gate('import');
  h.server.version = 2;
  await until(() => h.waiting.has('import'), 'the sync of version 2');
  assert.equal(h.imports().at(-1), 'import chat since 1');
  engine.dead = new Error('The engine worker failed: out of memory');
  engine.onfailed(engine.dead.message);
  held.resolve();
  await until(() => h.engines.length === 2 && h.engines[1].placed.length === 1, 'the next engine\'s placement');
  assert.deepEqual(h.engines[1].placed, [{ commands: WHOLE, fresh: true }], 'the whole world, asked for again');
  assert.equal(h.imports().at(-1), 'import chat');
  await until(() => live.state.status === 'running' && live.shownVersion === 2 && h.engines[1].ran().at(-1) === true, 'version 2 running');
});

test('a restart whose import failed is still asked for, with the world it would have replaced still held', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  h.importFails = 'HTTP 503';
  await live.control('restart');
  await until(() => live.phase === 'failed', 'the failed restart');
  assert.equal(live.state.revision, 1, 'the engine\'s world is still the one placed from version 1');
  assert.equal(live.restarting, true, 'the restart is not lost');
  const tries = h.imports().length;
  await pause(60);
  assert.equal(h.imports().length, tries, 'it is not tried again on its own');
  // Asked again, it is tried again, once.
  await live.control('restart');
  await until(() => h.imports().length === tries + 1 && live.phase === 'failed', 'the second try');
  await pause(60);
  assert.equal(h.imports().length, tries + 1);
  // The next version's sync carries it out: the whole world, placed afresh.
  h.importFails = null;
  h.server.version = 2;
  await until(() => live.state.status === 'running' && live.shownVersion === 2, 'version 2');
  assert.equal(h.imports().at(-1), 'import chat');
  assert.deepEqual(engine.placed.map(({ fresh }) => fresh), [true, true]);
  assert.equal(live.restarting, false, 'carried out');
  await pause(60);
  assert.equal(h.imports().length, tries + 2, 'once');
});

test('the world on show is needed to export, input or play, and a frame with nothing since the base carries no patch', async (t) => {
  const { live, h } = open(t);
  await assert.rejects(live.exportWorld('x'), /no live world on show/);
  await assert.rejects(live.control('play'), /no live world on show/);
  await running(live, h);
  const behind = live.accumulated();
  assert.equal(behind.patch, null);
  assert.deepEqual(behind.entities, []);
});

test('a removal waits for the spawn before it, and with no player asks nothing of the engine', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  await live.removePlayer();
  assert.ok(!engine.calls.some(([type]) => type === 'removePlayer'), 'no player, nothing removed');
  const spawning = engine.gate('spawn');
  const spawned = live.spawnPlayer({ x: 3 });
  const removed = live.removePlayer();
  await pause(10);
  assert.ok(!engine.calls.some(([type]) => type === 'removePlayer'), 'the removal waits');
  spawning.resolve();
  await spawned;
  await removed;
  assert.deepEqual(engine.calls.filter(([type]) => type === 'spawn' || type === 'removePlayer').map(([type]) => type), ['spawn', 'removePlayer']);
  assert.equal(live.playerView, null);
});

test('a spawn answered after a switch of view leaves no player in the new view, and input for no player goes nowhere', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h, 'a');
  const spawning = engine.gate('spawn');
  const spawned = live.spawnPlayer({ x: 3 });
  await until(() => engine.waiting.has('spawn'), 'the spawn');
  const switching = live.show('b');
  spawning.resolve();
  await assert.rejects(spawned, /The world let the player go before it stood in it/);
  await switching;
  assert.equal(live.playerView, null);
  await live.setPlayerInput({ keys: 1, taps: 0, yaw: 0, pitch: 0 });
  assert.ok(!engine.calls.some(([type]) => type === 'playerInput'));
  await live.removePlayer();
  assert.ok(!engine.calls.some(([type]) => type === 'removePlayer'));
});

test('the world letting its player go on its own is told, a removal is not', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  let gone = 0;
  live.onPlayerGone(() => { gone += 1; });
  await live.spawnPlayer({ x: 3 });
  await live.removePlayer();
  assert.equal(gone, 0, 'the user\'s leave is not the world\'s');
  await live.spawnPlayer({ x: 3 });
  assert.ok(live.playerView);
  await live.control('restart');
  await until(() => engine.calls.filter(([type, fresh]) => type === 'place' && fresh).length === 2 && live.state.status === 'running', 'placed afresh');
  assert.equal(live.playerView, null);
  assert.equal(gone, 1, 'a fresh placement took it');
  await live.spawnPlayer({ x: 4 });
  engine.dead = new Error('The engine worker failed: out of memory');
  engine.onfailed(engine.dead.message);
  assert.equal(gone, 2, 'so did the engine\'s end');
  assert.equal(live.engine, null);
});

test('input asked while a fresh placement is on its way reaches no player: the world lets it go first', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  let gone = 0;
  live.onPlayerGone(() => { gone += 1; });
  await live.spawnPlayer({ x: 3 });
  const placing = engine.gate('place');
  await live.control('restart');
  await until(() => engine.waiting.has('place'), 'the fresh placement');
  assert.equal(gone, 1, 'let go as the placement is asked');
  assert.equal(live.playerView, null);
  // A tick the engine ran before the placement still carries the player.
  engine.onframe({ ticks: 7, bytes: patchBytes(), entities: '[]', view: new Float64Array(28) });
  assert.equal(live.playerView, null, 'a tick of the player let go does not stand it in the world again');
  // The engine drops its player with the placement: input for it would fail.
  await live.setPlayerInput({ keys: 1, taps: 0, yaw: 0, pitch: 0 });
  assert.ok(!engine.calls.some(([type]) => type === 'playerInput'), 'no input for the player the placement takes');
  placing.resolve();
  await until(() => live.state.status === 'running' && engine.ran().length === 2, 'running again');
  assert.equal(gone, 1);
});

test('a respawn answered after the world let its player go stands no one: the placement that followed it took it', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  await live.spawnPlayer({ x: 3 });
  // The respawn reaches the engine before the fresh placement, and its answer
  // comes after the world let the player go.
  const spawning = engine.gate('spawn');
  const respawned = live.spawnPlayer({ x: 4 });
  await until(() => engine.waiting.has('spawn'), 'the respawn');
  await live.control('restart');
  await until(() => engine.calls.filter(([type, fresh]) => type === 'place' && fresh).length === 2, 'the fresh placement');
  // The viewer, told the player is gone, leaves player mode and sends its leave.
  const removed = live.removePlayer();
  spawning.resolve();
  await assert.rejects(respawned, /The world let the player go before it stood in it/);
  await removed;
  assert.equal(live.playerView, null);
  assert.equal(engine.player, false);
  assert.deepEqual(engine.calls.filter(([type]) => type === 'spawn' || type === 'removePlayer').map(([type]) => type), ['spawn', 'spawn'], 'no player to take out');
});

test('a first spawn a fresh placement takes is let go and told: the page and the engine agree no player stands', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  let gone = 0;
  live.onPlayerGone(() => { gone += 1; });
  // The spawn reaches the engine, then the fresh placement, which takes the
  // player; the spawn's answer comes after the placement was asked.
  const spawning = engine.gate('spawn');
  const spawned = live.spawnPlayer({ x: 3 });
  await until(() => engine.waiting.has('spawn'), 'the spawn');
  await live.control('restart');
  await until(() => engine.calls.filter(([type, fresh]) => type === 'place' && fresh).length === 2, 'the fresh placement');
  assert.equal(gone, 1, 'the viewer is told, and leaves player mode');
  spawning.resolve();
  await assert.rejects(spawned, /The world let the player go before it stood in it/);
  assert.equal(live.player, 'none');
  assert.equal(live.playerView, null);
  assert.equal(engine.player, false, 'the engine holds none either');
  // Input goes nowhere, not to an engine that would refuse it.
  await live.setPlayerInput({ keys: 1, taps: 0, yaw: 0, pitch: 0 });
  assert.ok(!engine.calls.some(([type]) => type === 'playerInput'));
  // A spawn after the placement stands in the new world.
  await until(() => live.state.status === 'running', 'placed');
  await live.spawnPlayer({ x: 5 });
  assert.equal(live.player, 'standing');
  assert.equal(engine.player, true);
  assert.equal(gone, 1);
});

test('a first spawn that fails stands no one, and the player goes only where its states allow', async (t) => {
  const { live, h } = open(t);
  const engine = await running(live, h);
  const spawning = engine.gate('spawn');
  const spawned = live.spawnPlayer({ x: 3 });
  await until(() => engine.waiting.has('spawn'), 'the spawn');
  spawning.reject(new Error('The engine failed: unported: a block'));
  await assert.rejects(spawned, /unported: a block/);
  assert.equal(live.player, 'none');
  assert.throws(() => live.playerTo('standing', new Float64Array(28)), /cannot go from none to standing/);
  assert.throws(() => live.playerTo('spawning', new Float64Array(28)), /A standing player, and only one, has a view/);
});
