'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { EventEmitter, PassThrough } = require('stream');
const { Engine } = require('../server/engine');

// An engine whose program is a fake child: the test reads its requests and
// writes its replies.
function fakeEngine() {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-'));
  const children = [];
  const builds = [];
  const engine = new Engine({
    build: async (options) => { builds.push(options); return '/engine/mcbench-engine-test'; },
    spawnEngine: (binary, args, options) => {
      const child = new EventEmitter();
      child.binary = binary;
      child.options = options;
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.stdin = new PassThrough();
      child.requests = [];
      child.stdin.on('data', (chunk) => child.requests.push(...String(chunk).trim().split('\n')));
      child.exitCode = null;
      child.kill = () => { child.exitCode = 0; child.emit('close', 0, null); };
      children.push(child);
      return child;
    },
  });
  return { engine, children, builds, folder };
}

const turn = () => new Promise((resolve) => setImmediate(resolve));

test('the engine is built for the server and runs placements and experiments', async () => {
  const { engine, children, builds, folder } = fakeEngine();
  const ready = engine.warm({ classpath: ['/paper.jar'], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (!children.length) await turn();
  // The program is built where every engine process finds it; this one runs in its own folder.
  assert.deepEqual(builds.map(({ signal, ...options }) => options), [{ classpath: ['/paper.jar'], folder: '/engine', minecraftVersion: '1.21.10' }]);
  // The engine's world holds the live server's data packs.
  assert.ok(fs.existsSync(path.join(folder, 'world', 'datapacks', 'mcbench', 'data', 'minecraft', 'dimension_type', 'overworld.json')));
  assert.equal(children[0].binary, '/engine/mcbench-engine-test');
  assert.equal(children[0].options.cwd, folder);
  children[0].stdout.write('MCBENCH_ENGINE_READY\n');
  await ready;

  const placed = engine.place('/r/commands.txt', '/r/blueprint.bin', { fresh: false });
  while (!children[0].requests.length) await turn();
  assert.equal(children[0].requests[0], 'PLACE 1 /r/commands.txt /r/blueprint.bin continue');
  children[0].stdout.write('MCBENCH_PLACED 1 3\n');
  await placed;

  const verified = engine.verify('/r/request.json', '/r/commands.txt', '/r/out');
  while (children[0].requests.length < 2) await turn();
  assert.equal(children[0].requests[1], 'VERIFY 2 /r/request.json /r/commands.txt /r/out');
  children[0].stdout.write('MCBENCH_VERIFIED 2 12\n');
  await verified;

  const failed = engine.verify('/r/request.json', '/r/commands.txt', '/r/out');
  while (children[0].requests.length < 3) await turn();
  children[0].stdout.write('MCBENCH_VERIFY_FAILED 3 java.lang.UnsupportedOperationException: Light is not modelled\n');
  await assert.rejects(failed, /Light is not modelled/);

  // The ticks light what they changed unless the caller reads only blocks after them.
  const stock = engine.tick(100, '/r/patch', '/r/entities');
  while (children[0].requests.length < 4) await turn();
  assert.equal(children[0].requests[3], 'TICK 4 100 /r/patch /r/entities');
  children[0].stdout.write('MCBENCH_TICKED 4 3\n');
  assert.equal(await stock, undefined);
  const dark = engine.tick(100, '/r/patch', '/r/entities', { light: false });
  while (children[0].requests.length < 5) await turn();
  assert.equal(children[0].requests[4], 'TICK 5 100 /r/patch /r/entities nolight');
  children[0].stdout.write('MCBENCH_TICKED 5 3\n');
  assert.equal(await dark, undefined);
  await engine.stop();
});

test('an engine that cannot start reports why', async () => {
  const { engine, children, folder } = fakeEngine();
  const ready = engine.warm({ classpath: ['/paper.jar'], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (!children.length) await turn();
  children[0].stdout.write('MCBENCH_ENGINE_FAILED java.lang.IllegalStateException: no registries\n');
  children[0].emit('close', 1, null);
  await assert.rejects(ready, /no registries/);
});

test('a failed build reaches the caller and the next warm builds again', async () => {
  let attempts = 0;
  const engine = new Engine({ build: async () => { attempts += 1; throw new Error('native-image ran out of memory'); } });
  await assert.rejects(engine.warm({ classpath: [], binaries: '/engine', folder: '/engine/run', minecraftVersion: '1.21.10' }), /out of memory/);
  await assert.rejects(engine.warm({ classpath: [], binaries: '/engine', folder: '/engine/run', minecraftVersion: '1.21.10' }), /out of memory/);
  assert.equal(attempts, 2);
});

test('stopping the engine while it is being built stops the build', async () => {
  let signal;
  let building;
  const engine = new Engine({
    build: (options) => new Promise((resolve, reject) => {
      signal = options.signal;
      building = true;
      signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    }),
  });
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-'));
  const ready = engine.warm({ classpath: [], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (!building) await turn();
  await engine.stop();
  assert.equal(signal.aborted, true);
  await assert.rejects(ready, /stopped while it was being built/);
});

test('an engine stopped while it is built never spawns, and its requests fail plainly', async () => {
  let release;
  const spawned = [];
  // A build that ignores its signal, as cargo's shared build does.
  const engine = new Engine({ build: () => new Promise((resolve) => { release = resolve; }), spawnEngine: () => { spawned.push(1); throw new Error('spawned'); } });
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-'));
  const ready = engine.warm({ folder });
  await turn();
  await engine.stop();
  await assert.rejects(ready, /stopped while it was being built/);
  release('/engine/mcbench-core');
  await turn();
  assert.deepEqual(spawned, []);
  assert.equal(engine.alive, false);
  await assert.rejects(engine.place('/r/c.txt', '/r/b.bin'), /The engine was stopped/);
  await assert.rejects(new Engine().place('/r/c.txt', '/r/b.bin'), /The engine was not started/);
});

test('a dead engine fails requests with its own stderr, and a restart starts with fresh output', async () => {
  const { engine, children, folder } = fakeEngine();
  const ready = engine.warm({ classpath: [], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (!children.length) await turn();
  children[0].stdout.write('MCBENCH_ENGINE_READY\n');
  await ready;
  const pending = engine.place('/r/c.txt', '/r/b.bin');
  while (!children[0].requests.length) await turn();
  children[0].stderr.write('first process ran out of memory');
  await turn();
  children[0].emit('close', null, 'SIGKILL');
  await assert.rejects(pending, /SIGKILL.*first process ran out of memory/);
  assert.equal(engine.alive, false);
  await assert.rejects(engine.tick(1, '/r/p', '/r/e'), /The engine stopped \(SIGKILL\): first process ran out of memory/);
  // The next warm starts a second process, whose failure says only its own output.
  const again = engine.warm({ classpath: [], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (children.length < 2) await turn();
  children[1].stderr.write('second');
  await turn();
  children[1].emit('close', 1, null);
  await assert.rejects(again, (error) => /code 1\): second$/.test(error.message));
});

test('an old process closing late leaves the new process\'s requests alone', async () => {
  const { engine, children, folder } = fakeEngine();
  const options = { classpath: [], binaries: '/engine', folder, minecraftVersion: '1.21.10' };
  // The first process ignores SIGTERM for a while.
  const first = engine.warm(options);
  while (!children.length) await turn();
  children[0].kill = (signal) => { children[0].signals = [...(children[0].signals || []), signal]; };
  children[0].stdout.write('MCBENCH_ENGINE_READY\n');
  await first;
  const stopping = engine.stop();
  const second = engine.warm(options);
  while (children.length < 2) await turn();
  children[1].stdout.write('MCBENCH_ENGINE_READY\n');
  await second;
  const placed = engine.place('/r/c.txt', '/r/b.bin');
  while (!children[1].requests.length) await turn();
  children[0].exitCode = null;
  children[0].signalCode = 'SIGTERM';
  children[0].emit('close', null, 'SIGTERM');
  await stopping;
  // It died of SIGTERM: no SIGKILL follows.
  assert.deepEqual(children[0].signals, ['SIGTERM']);
  children[1].stdout.write(`MCBENCH_PLACED ${children[1].requests[0].split(' ')[1]} 1\n`);
  await placed;
  assert.equal(engine.alive, true);
  await engine.stop();
});

test('a request argument holding whitespace is refused before it reaches the engine', async () => {
  const { engine, children, folder } = fakeEngine();
  const ready = engine.warm({ classpath: [], binaries: '/engine', folder, minecraftVersion: '1.21.10' });
  while (!children.length) await turn();
  children[0].stdout.write('MCBENCH_ENGINE_READY\n');
  await ready;
  await assert.rejects(engine.place('/r/my commands.txt', '/r/b.bin'), /cannot be empty or hold whitespace: "\/r\/my commands.txt"/);
  assert.deepEqual(children[0].requests, []);
  await engine.stop();
});

test('a caller whose signal already aborted does not join a shared native build', () => {
  const engineModule = require('../server/engine');
  const aborted = new AbortController();
  aborted.abort(new Error('gone'));
  assert.throws(() => engineModule.buildEngine({ classpath: [], folder: '/nowhere', minecraftVersion: '1.21.10', signal: aborted.signal }), /gone/);
});
