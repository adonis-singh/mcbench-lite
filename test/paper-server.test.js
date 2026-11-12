'use strict';

const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { EventEmitter } = require('node:events');
const { test } = require('node:test');

const { PaperServer } = require('../server/paper-server');
const { World } = require('../server/world');

// A zip of uncompressed entries.
function storedZip(entries) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of Object.entries(entries)) {
    const data = Buffer.from(text);
    const fileName = Buffer.from(name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(zlib.crc32(data), 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(fileName.length, 26);
    const record = Buffer.alloc(46);
    record.writeUInt32LE(0x02014b50, 0);
    record.writeUInt16LE(20, 4);
    record.writeUInt16LE(20, 6);
    record.writeUInt32LE(zlib.crc32(data), 16);
    record.writeUInt32LE(data.length, 20);
    record.writeUInt32LE(data.length, 24);
    record.writeUInt16LE(fileName.length, 28);
    record.writeUInt32LE(offset, 42);
    locals.push(local, fileName, data);
    central.push(record, fileName);
    offset += local.length + fileName.length + data.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(entries).length, 8);
  end.writeUInt16LE(Object.keys(entries).length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

const PAPER_FILES = ['versions/1.21.10/paper-1.21.10.jar', 'libraries/com/example/a.jar', 'libraries/org/example/b.jar'];

// Paper's jar as downloaded, naming its server jar and libraries.
function writePaperJar(paperDir) {
  fs.writeFileSync(path.join(paperDir, 'paper-1.21.10-130.jar'), storedZip({
    'META-INF/versions.list': 'hash\t1.21.10\t1.21.10/paper-1.21.10.jar\n',
    'META-INF/libraries.list': 'hash\tcom.example:a:1\tcom/example/a.jar\nhash\torg.example:b:1\torg/example/b.jar\n',
  }));
}

// The files Paper's launcher lays out in patch-only mode.
function layOutPaper(paperDir) {
  for (const file of PAPER_FILES) {
    fs.mkdirSync(path.dirname(path.join(paperDir, file)), { recursive: true });
    fs.writeFileSync(path.join(paperDir, file), file);
  }
}

// Stand-ins for Paper's process (a ChildProcess): each prints its startup
// line, and exits when signalled, after `onSignal` (where Java would write an
// AOT cache). A server that `ignoresTerm` only exits on SIGKILL.
function fakeServers({ output = '[Server thread/INFO]: Done (1.0s)! For help, type "help"', ignoresTerm = false, onSignal = () => {} } = {}) {
  const started = [];
  const spawnServer = (args, options) => {
    const server = Object.assign(new EventEmitter(), {
      args, options, pid: 40000 + started.length, signals: [], exitCode: null, signalCode: null,
      stdout: new EventEmitter(), stderr: new EventEmitter(),
      exit(code, signal = null) {
        server.exitCode = code;
        server.signalCode = signal;
        server.emit('exit', code, signal);
      },
      kill(signal) {
        server.signals.push(signal);
        if (signal === 'SIGTERM' && ignoresTerm) return true;
        setImmediate(() => {
          onSignal(server, signal);
          if (signal === 'SIGKILL') server.exit(null, 'SIGKILL');
          else server.exit(143);
        });
        return true;
      },
    });
    started.push(server);
    if (output) queueMicrotask(() => server.stdout.emit('data', output));
    return server;
  };
  return { started, spawnServer };
}

function serverHarness(options) {
  const paperDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-lite-paper-test-'));
  // Paper as if downloaded and laid out already; these tests never boot it.
  writePaperJar(paperDir);
  layOutPaper(paperDir);
  const servers = fakeServers();
  return {
    servers,
    paperDir,
    aotDir: path.join(paperDir, 'test-aot'),
    spawnServer: servers.spawnServer,
    findJavaBuild: () => 'test-java',
    connectRcon: async () => ({ async exec() { return ''; }, close() {} }),
    runJava: async () => {},
    ...options,
  };
}

test('the first run records an AOT cache that later runs load, and patching runs once', async () => {
  const javaCalls = [];
  const servers = fakeServers({
    // Java writes the recorded cache as it exits.
    onSignal: (server) => {
      const output = server.args.find((arg) => arg.startsWith('-XX:AOTCacheOutput='));
      if (output) fs.writeFileSync(output.split('=')[1], 'cache');
    },
  });
  const harness = serverHarness({
    runJava: async (args, options) => {
      javaCalls.push({ args, cwd: options.cwd });
      if (args.includes('-Dpaperclip.patchonly=true')) layOutPaper(harness.paperDir);
    },
    spawnServer: servers.spawnServer,
    wait: async () => {},
  });
  // A fresh install: the downloaded jar only, and a lobby from an earlier run.
  for (const file of ['versions', 'libraries']) fs.rmSync(path.join(harness.paperDir, file), { recursive: true });
  fs.mkdirSync(path.join(harness.paperDir, 'world'));
  // What earlier runs left: the launcher's vanilla jar, caches for other
  // JVMs (one used lately, one not for weeks), a killed run's recording, and
  // a dozen archived logs.
  fs.mkdirSync(path.join(harness.paperDir, 'cache'));
  fs.writeFileSync(path.join(harness.paperDir, 'cache', 'mojang_1.21.10.jar'), 'vanilla');
  fs.mkdirSync(harness.aotDir);
  const daysAgo = (days) => new Date(Date.now() - days * 86_400_000);
  for (const [name, days] of [['1111111111111111.aot', 1], ['0000000000000000.aot', 20], ['2222222222222222.recording.aot', 2]]) {
    fs.writeFileSync(path.join(harness.aotDir, name), 'old');
    fs.utimesSync(path.join(harness.aotDir, name), daysAgo(days), daysAgo(days));
  }
  fs.mkdirSync(path.join(harness.paperDir, 'logs'));
  for (let day = 1; day <= 12; day += 1) {
    const log = path.join(harness.paperDir, 'logs', `2026-10-${String(day).padStart(2, '0')}-1.log.gz`);
    fs.writeFileSync(log, '');
    fs.utimesSync(log, day * 86_400, day * 86_400);
  }
  const live = new PaperServer({ ...harness });

  await live.startServer();
  assert.strictEqual(fs.existsSync(path.join(harness.paperDir, 'cache')), false);
  assert.deepStrictEqual(fs.readdirSync(harness.aotDir), ['1111111111111111.aot']);
  // Every copy of Paper's classpath carries one time, so a shared cache matches it.
  assert.ok(PAPER_FILES.every((file) => fs.statSync(path.join(harness.paperDir, file)).mtimeMs === Date.UTC(2000, 0, 1)));
  assert.deepStrictEqual(fs.readdirSync(path.join(harness.paperDir, 'logs')).sort()[0], '2026-10-03-1.log.gz');
  assert.strictEqual(fs.readdirSync(path.join(harness.paperDir, 'logs')).length, 10);
  // Paper's launcher lays out its files once, on the host's Java.
  assert.deepStrictEqual(javaCalls, [{ args: ['-Dpaperclip.patchonly=true', '-jar', 'paper-1.21.10-130.jar'], cwd: harness.paperDir }]);
  assert.ok(fs.existsSync(path.join(harness.paperDir, 'world')));
  const recording = servers.started[0].args.find((arg) => arg.startsWith('-XX:AOTCacheOutput='));
  assert.match(recording, new RegExp(`^-XX:AOTCacheOutput=${harness.aotDir}/[0-9a-f]{16}\\.recording\\.aot$`));

  // The recording run stops gracefully (SIGTERM, then waits for Java to exit
  // and write the cache), and the recording becomes the cache.
  await live.stop();
  assert.deepStrictEqual(servers.started[0].signals, ['SIGTERM']);
  const cache = recording.split('=')[1].replace('.recording.aot', '.aot');
  assert.ok(fs.existsSync(cache));
  assert.ok(!fs.existsSync(recording.split('=')[1]));
  assert.strictEqual(fs.existsSync(path.join(harness.paperDir, 'mcbench-server.pid')), false);

  await live.startServer();
  assert.strictEqual(javaCalls.length, 1);
  const cached = servers.started[1];
  assert.deepStrictEqual(cached.args.filter((arg) => arg.startsWith('-XX:AOT')), ['-XX:AOTMode=on', `-XX:AOTCache=${cache}`]);
  await live.stop();
  assert.deepStrictEqual(cached.signals, ['SIGTERM']);
});

test('a recording server is given the long graceful stop; any other that ignores SIGTERM is killed', async () => {
  const servers = fakeServers({ ignoresTerm: true });
  const harness = serverHarness({ spawnServer: servers.spawnServer, wait: async () => {}, stopMs: 20 });
  const live = new PaperServer(harness);
  await live.startServer();
  const recording = servers.started[0].args.find((arg) => arg.startsWith('-XX:AOTCacheOutput=')).split('=')[1];
  assert.strictEqual(live.state.training, recording.replace('.recording.aot', '.aot'));
  // The recording run is given the long graceful stop: not killed after stopMs.
  const stopping = live.stop();
  await new Promise((resolve) => setTimeout(resolve, 100));
  assert.deepStrictEqual(servers.started[0].signals, ['SIGTERM']);
  servers.started[0].exit(143);
  await stopping;
  // Without a recording, SIGKILL follows after stopMs.
  fs.writeFileSync(live.aotCache([`-Xms${live.memory}`, `-Xmx${live.memory}`]), 'cache');
  await live.startServer();
  assert.strictEqual(live.state.training, null);
  await live.stop();
  assert.deepStrictEqual(servers.started[1].signals, ['SIGTERM', 'SIGKILL']);
  assert.strictEqual(servers.started[1].signalCode, 'SIGKILL');
  assert.strictEqual(live.server, null);
});

test('startup waits for the ready line across polls', async t => {
  let polls = 0;
  const servers = fakeServers({ output: 'Starting...\n' });
  const options = serverHarness({
    spawnServer: servers.spawnServer,
    // The ready line arrives on the third poll.
    wait: async () => { if (++polls === 3) servers.started[0].stdout.emit('data', '[Server thread/INFO]: Done (1.0s)! For help'); },
  });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  assert.equal(polls, 3);
  assert.equal(live.state.name, 'ready');
  assert.ok(live.state.rcon);
});

test('a server that exits during startup fails the start with its output', async t => {
  const servers = fakeServers({ output: '' });
  // Each poll lets the test run between them.
  const options = serverHarness({ spawnServer: servers.spawnServer, wait: () => new Promise((resolve) => setImmediate(resolve)) });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  const starting = live.startServer();
  while (!servers.started.length) await new Promise((resolve) => setImmediate(resolve));
  servers.started[0].stderr.emit('data', 'Error: Unable to access jarfile');
  servers.started[0].exit(1);
  await assert.rejects(starting, /exited during startup \(code 1\): Error: Unable to access jarfile/);
});

test('with game port 0 the server gets a free port of its own', async t => {
  const options = serverHarness({ gamePort: 0, wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  const properties = fs.readFileSync(path.join(live.paperDir, 'server.properties'), 'utf8');
  const port = Number(properties.match(/^server-port=(\d+)$/m)[1]);
  assert.ok(port > 0 && port !== 25599);
});

test('reapStray ends only a java process of this folder that an earlier run recorded', async t => {
  const options = serverHarness({ wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const pidFile = path.join(options.paperDir, 'mcbench-server.pid');
  const java = (folder) => `/jdk/bin/java -Xms4G -Xmx4G -Dmcbench.server=${folder} -cp a org.bukkit.craftbukkit.Main --nogui`;
  const reap = async (command, { ends = true } = {}) => {
    const signals = [];
    let alive = true;
    fs.writeFileSync(pidFile, '4242');
    await new PaperServer({
      ...options,
      inspectProcess: async (pid) => { assert.equal(pid, 4242); return alive ? command : null; },
      killProcess: (pid, signal) => { signals.push(signal); if (ends) alive = false; },
    }).reapStray();
    assert.equal(fs.existsSync(pidFile), false);
    return signals;
  };
  assert.deepEqual(await reap(java(options.paperDir)), ['SIGTERM']);
  assert.deepEqual(await reap(java(`${options.paperDir}-other`)), [], 'a java process of another folder');
  assert.deepEqual(await reap(`/usr/bin/node -Dmcbench.server=${options.paperDir} x`), [], 'another program');
  assert.deepEqual(await reap(null), [], 'no such process');
  // A server that ignores SIGTERM is killed once its time is up.
  const live = new PaperServer({ ...options, inspectProcess: async () => java(options.paperDir), killProcess: (pid, signal) => { signals.push(signal); } });
  const signals = [];
  fs.writeFileSync(pidFile, '4242');
  const clock = Date.now;
  let now = clock();
  Date.now = () => (now += 10_000);
  try { await live.reapStray(); } finally { Date.now = clock; }
  assert.deepEqual(signals, ['SIGTERM', 'SIGKILL']);
  // Nothing recorded, nothing to do.
  await new PaperServer({ ...options, inspectProcess: async () => assert.fail('inspected') }).reapStray();
});

test('a world is applied as ordered commands, with every column and its margin loaded first and ticks frozen meanwhile', async (t) => {
  const commands = [];
  const options = serverHarness({
    connectRcon: async () => ({ async exec(command) { commands.push(command); return ''; }, close() {} }),
    wait: async () => {},
  });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  commands.length = 0;
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  world.set(1, 0, 0, 'minecraft:oak_planks', {});
  world.finalize();
  // One by one over RCON (the commands are tiny), the columns around chunk 0, 0 first.
  const keys = await live.applyWorld(world);
  assert.strictEqual(keys.length, 1);
  assert.strictEqual(commands[0], 'tick freeze');
  assert.strictEqual(commands.filter((command) => command.startsWith('forceload add')).length, 9);
  assert.ok(commands.indexOf('setblock 0 64 0 minecraft:stone') > commands.lastIndexOf('forceload add -16 -16'));
  assert.ok(commands.includes('setblock 1 64 0 minecraft:oak_planks'));
  assert.strictEqual(commands.at(-1), 'tick unfreeze');
  // A second world, two chunks over, keeps the column it shares with the first and drops the others.
  commands.length = 0;
  const next = new World();
  next.set(40, 0, 0, 'minecraft:stone', {});
  next.finalize();
  await live.applyWorld(next);
  assert.strictEqual(commands.filter((command) => command.startsWith('forceload remove')).length, 6);
});

test('a server that crashes after its start is dead: no command reaches it and it is not alive', async (t) => {
  const options = serverHarness({ wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  let closed = false;
  options.connectRcon = async () => ({ closed: false, async exec() { return ''; }, close() { closed = true; this.closed = true; } });
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  assert.equal(live.alive, true);
  options.servers.started[0].exit(1);
  assert.equal(live.alive, false);
  assert.equal(closed, true);
  await assert.rejects(live.command('say hi'), /The Minecraft server exited \(code 1\)/);
});

test('a start that stop() overtook while it prepared never spawns a server', async (t) => {
  let release;
  const options = serverHarness({ wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  const ensurePaper = live.ensurePaper.bind(live);
  live.ensurePaper = () => new Promise((resolve) => { release = () => resolve(ensurePaper()); });
  const starting = live.startServer();
  while (!release) await new Promise((resolve) => setImmediate(resolve));
  await live.stop();
  release();
  await assert.rejects(starting, /The Paper server was stopped/);
  assert.equal(options.servers.started.length, 0);
});

test('a start that fails ends the process it spawned', async (t) => {
  const servers = fakeServers({ output: 'You need to agree to the EULA in order to run the server' });
  const options = serverHarness({ spawnServer: servers.spawnServer, wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  await assert.rejects(live.startServer(), /rejected the EULA/);
  assert.deepEqual(servers.started[0].signals, ['SIGTERM']);
  assert.equal(live.server, null);
  assert.equal(fs.existsSync(path.join(options.paperDir, 'mcbench-server.pid')), false);
});

test('a recording server that crashed does not leave its partial recording as the cache', async (t) => {
  const servers = fakeServers();
  const options = serverHarness({ spawnServer: servers.spawnServer, wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  await live.startServer();
  const recording = servers.started[0].args.find((arg) => arg.startsWith('-XX:AOTCacheOutput=')).split('=')[1];
  fs.writeFileSync(recording, 'partial');
  servers.started[0].exit(134);
  await live.stop();
  assert.equal(fs.existsSync(recording.replace('.recording.aot', '.aot')), false);
});

test('a placement that failed part-way leaves the world unknown until a fresh one', async (t) => {
  let failing = false;
  const options = serverHarness({
    connectRcon: async () => ({ async exec(command) { if (failing && command.startsWith('setblock')) throw new Error('RCON connection closed'); return ''; }, close() {} }),
    wait: async () => {},
  });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  world.finalize();
  failing = true;
  await assert.rejects(live.applyWorld(world), /RCON connection closed/);
  assert.equal(live.appliedWorld, null);
  failing = false;
  await assert.rejects(live.applyWorld(world), /failed part-way/);
  live.freshWorld('minecraft:overworld');
  await live.applyWorld(world);
  assert.equal(live.appliedWorld, world);
});

test('a server an earlier run left (named in the pid file) is ended before a new one starts', async (t) => {
  const signals = [];
  let alive = true;
  const options = serverHarness({ wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  fs.writeFileSync(path.join(options.paperDir, 'mcbench-server.pid'), '4242');
  const live = new PaperServer({
    ...options,
    inspectProcess: async () => (alive ? `/jdk/bin/java -Dmcbench.server=${options.paperDir} -cp a org.bukkit.craftbukkit.Main` : null),
    killProcess: (pid, signal) => { signals.push([pid, signal]); alive = false; },
  });
  t.after(() => live.stop());
  await live.startServer();
  assert.deepEqual(signals, [[4242, 'SIGTERM']]);
  assert.equal(fs.readFileSync(path.join(options.paperDir, 'mcbench-server.pid'), 'utf8'), String(options.servers.started[0].pid));
});

test('a start during another, or of a running server, is refused: one server is spawned', async (t) => {
  const options = serverHarness({ wait: async () => {} });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  const first = live.startServer();
  await assert.rejects(live.startServer(), /The Paper server is starting and cannot be started/);
  await first;
  await assert.rejects(live.startServer(), /The Paper server is ready and cannot be started/);
  assert.equal(options.servers.started.length, 1);
  assert.equal(live.alive, true);
  assert.equal(fs.readFileSync(path.join(options.paperDir, 'mcbench-server.pid'), 'utf8'), String(options.servers.started[0].pid));
  // Nor while it is being stopped; once stopped, it starts again.
  const stopping = live.stop();
  await assert.rejects(live.startServer(), /The Paper server is stopping and cannot be started/);
  await stopping;
  await live.startServer();
  assert.equal(options.servers.started.length, 2);
});

test('a server that died starts again with nothing of its last world placed or loaded', async (t) => {
  const commands = [];
  const options = serverHarness({
    connectRcon: async () => ({ async exec(command) { commands.push(command); return ''; }, close() {} }),
    wait: async () => {},
  });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  await live.startServer();
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  world.finalize();
  live.freshWorld('minecraft:mcbench_test');
  await live.applyWorld(world);
  options.servers.started[0].exit(1);
  assert.equal(live.alive, false);
  commands.length = 0;
  await live.startServer();
  assert.equal(options.servers.started.length, 2);
  assert.equal(live.alive, true);
  // The new process is told what a first one is: nothing in the dimension of the world that is gone.
  assert.equal(commands[0], 'defaultgamemode spectator');
  assert.equal(live.appliedWorld, null);
  assert.equal(live.loadedColumns.size, 0);
  // The same world is placed whole, not as no change from the one before.
  commands.length = 0;
  await live.applyWorld(world);
  assert.equal(commands.filter((command) => command.startsWith('forceload add')).length, 9);
  assert.ok(commands.includes('setblock 0 64 0 minecraft:stone'));
});

test('the state says where the server is: booting is not a lost RCON connection, and no start is left starting', async (t) => {
  let release;
  let failing = true;
  const options = serverHarness({
    connectRcon: () => new Promise((resolve) => { release = () => resolve({ closed: false, async exec() { return ''; }, close() { this.closed = true; } }); }),
    wait: async () => {},
  });
  t.after(() => fs.rmSync(options.paperDir, { recursive: true, force: true }));
  const live = new PaperServer(options);
  t.after(() => live.stop());
  assert.equal(live.state.name, 'stopped');
  // A start that fails before it spawns leaves the server stopped, to be started again.
  const ensurePaper = live.ensurePaper.bind(live);
  live.ensurePaper = async () => { if (failing) throw new Error('no Paper'); return ensurePaper(); };
  await assert.rejects(live.startServer(), /no Paper/);
  assert.equal(live.state.name, 'stopped');
  failing = false;
  const starting = live.startServer();
  assert.equal(live.state.name, 'starting');
  assert.equal(live.server, null);
  while (!release) await new Promise((resolve) => setImmediate(resolve));
  assert.equal(live.state.name, 'booting');
  assert.equal(live.server, options.servers.started[0]);
  assert.equal(live.alive, false);
  await assert.rejects(live.command('say hi'), /Minecraft RCON is not connected/);
  release();
  await starting;
  assert.equal(live.state.name, 'ready');
  assert.equal(live.alive, true);
  // RCON lost: the process runs, and the server is ready no more than in name.
  live.state.rcon.close();
  assert.equal(live.state.name, 'ready');
  assert.equal(live.alive, false);
  await assert.rejects(live.command('say hi'), /Minecraft RCON is not connected/);
  const stopping = live.stop();
  assert.equal(live.state.name, 'stopping');
  await assert.rejects(live.command('say hi'), /The Paper server was stopped/);
  await Promise.all([stopping, live.stop()]);
  assert.equal(live.state.name, 'stopped');
  assert.equal(live.server, null);
  assert.deepEqual(options.servers.started[0].signals, ['SIGTERM']);
});
