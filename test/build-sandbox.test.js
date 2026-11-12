'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const net = require('node:net');
const path = require('node:path');

const { runProgram, WALL_TIMEOUT_MS } = require('../server/build-sandbox');
const { runProgram: runInProcess, formatProgramError } = require('../server/mc-dsl');
const { World } = require('../server/world');

const REGISTRY = {
  oak_planks: [{}, {}],
  stone: [{}, {}],
  oak_stairs: [{ facing: ['north', 'south', 'east', 'west'], half: ['top', 'bottom'] }, { facing: 'north', half: 'bottom' }],
  torch: [{}, {}],
  wall_torch: [{ facing: ['north', 'south', 'east', 'west'] }, { facing: 'north' }],
};

const run = (src, world = new World(), readFile = null) => runProgram(src, world, REGISTRY, readFile);

test('the isolated run matches the in-process run block for block', async () => {
  const src = `module.exports = (mc) => {
    mc.fill([0, 0, 0], [8, 0, 8], 'oak_planks');
    mc.fill([0, 1, 0], [8, 5, 8], 'stone', 'hollow');
    mc.set([2, 6, 2], 'oak_stairs[facing=west,half=top]');
    mc.replace('stone', 'oak_planks', { from: [0, 1, 0], to: [1, 5, 1] });
    mc.set([4, 1, 4], 'air');
    mc.log('blocks', mc.count(), 'rand', Math.random());
    mc.writeFile('out.json', JSON.stringify({ n: mc.count() }));
  };`;
  const base = new World();
  base.fill([-3, -3, -3], [12, 8, 12], 'minecraft:oak_planks');
  base.finalize();

  const expectedWorld = base.clone();
  const expected = runInProcess(src, expectedWorld, REGISTRY, null);
  const actualWorld = base.clone();
  const actual = await runProgram(src, actualWorld, REGISTRY, null);

  assert.deepEqual(actual, expected);
  assert.deepEqual(actualWorld.diff(base), expectedWorld.diff(base));
  assert.equal(actualWorld.size(), expectedWorld.size());
  actualWorld.forEach((x, y, z, block) => assert.deepEqual(block, expectedWorld.get(x, y, z)));
  expectedWorld.forEach((x, y, z, block) => assert.deepEqual(block, actualWorld.get(x, y, z)));
});

test('a cloned world keeps copy-on-write sharing and an untouched base after an isolated run', async () => {
  const base = new World();
  base.fill([0, 0, 0], [40, 4, 40], 'minecraft:stone');
  base.finalize();
  const candidate = base.clone();
  await runProgram('module.exports = mc => mc.set([0, 5, 0], "oak_planks")', candidate, REGISTRY, null);
  assert.equal(candidate.size(), base.size() + 1);
  assert.deepEqual(candidate.diff(base), { added: 1, removed: 0, changed: 0 });
  assert.equal(base.get(0, 5, 0), null);
  // A chunk the program never touched is the same object in both worlds.
  const untouched = World.keyOf(2, 0, 2);
  assert.equal(candidate.chunks.get(untouched), base.chunks.get(untouched));
});

test('Math.random stays deterministic and matches the in-process seed', async () => {
  const src = 'module.exports = mc => mc.log(Math.random(), Math.random())';
  const a = (await run(src)).logs;
  const b = (await run(src)).logs;
  assert.deepEqual(a, b);
  assert.deepEqual(a, runInProcess(src, new World(), REGISTRY, null).logs);
});

test('mc.readFile is served over the channel and its errors reach the program', async () => {
  const served = await run('module.exports = mc => mc.log(mc.readFile("grid.txt"))', new World(), (relPath) => {
    assert.equal(relPath, 'grid.txt');
    return 'payload-from-host';
  });
  assert.deepEqual(served.logs, ['payload-from-host']);

  await assert.rejects(
    run('module.exports = mc => mc.readFile("missing")', new World(), () => { throw new Error('No such file in /work: missing'); }),
    /No such file in \/work: missing/,
  );
});

test('validation and error formatting are identical to the in-process runtime', async () => {
  await assert.rejects(run('module.exports = mc => mc.set([0,0,0], "oak_stairs[facing=up]")'), /facing=up is invalid/);
  await assert.rejects(run('module.exports = mc => mc.set([0,1,0], "torch")'), /floor torch at \[0, 1, 0\]/);
  await assert.rejects(run('module.exports = async mc => mc.set([0,0,0],"stone")'), /synchronous/);

  const src = "module.exports = (mc) => {\n  mc.log('phase 1 done');\n  const parts = [{ h: 2 }];\n  mc.set([0, parts[1].h, 0], 'stone');\n};";
  await assert.rejects(run(src), (err) => {
    assert.deepEqual(err.logs, ['phase 1 done']);
    const formatted = formatProgramError(err);
    assert.match(formatted, /Cannot read properties of undefined/);
    assert.match(formatted, /build\.js:4/);
    assert.doesNotMatch(formatted, /node:vm|bootstrap\.js|build-child\.js/);
    return true;
  });
});

// --- the boundary itself ------------------------------------------------------

// The confirmed vector: normVec3 calls raw.map on the model's own array, so a
// custom map receives a host-realm callback whose constructor is the host
// Function, which evaluates in the host realm and hands back its `process`.
const escapeToHostRealm = (body) => `module.exports = (mc) => {
  let hostFn;
  const arr = [0, 0, 0];
  arr.map = function (cb) { hostFn = cb; return [0, 0, 0]; };
  mc.set(arr, 'stone');
  const process = hostFn.constructor('return process')();
  ${body}
};`;

// Linux puts the build process in its own PID namespace (it sees itself as
// PID 1 with no parent) and under RLIMIT_DATA; macOS's seatbelt has neither,
// so there the process keeps its real pid and only the heap cap bounds memory.
const LINUX = process.platform === 'linux';
const PID = LINUX ? '1' : '\\d+';

test('the normVec3 map vector reaches the host realm of an empty, namespaced process', async () => {
  // The escape works: the program obtains the build process's real `process`.
  // What it finds there is PID 1 of its own namespace with no environment
  // (on Linux only bwrap's PWD, the / it starts in).
  const reached = await run(escapeToHostRealm(
    'mc.log(process.pid, process.ppid, JSON.stringify(Object.entries(process.env).filter(([k]) => k !== "MCBENCH_MC_Y_OFFSET")), process.permission.has("fs.read", "/"));',
  ));
  assert.match(reached.logs[0], LINUX ? /^1 0 \[\["PWD","\/"\]\] false$/ : /^\d+ \d+ \[\] false$/);
  assert.equal(reached.logs.length, 1);
});

test('an escaped program cannot read a canary outside /work', async () => {
  const canary = path.join(os.tmpdir(), `mcbench-canary-${process.pid}.txt`);
  fs.writeFileSync(canary, 'TOP-SECRET-REFERENCE-BUILD');
  try {
    const escaped = await run(escapeToHostRealm(`
      const fs = process.getBuiltinModule ? process.getBuiltinModule('fs') : null;
      let outcome = 'no-fs';
      try { outcome = fs.readFileSync(${JSON.stringify(canary)}, 'utf8'); } catch (e) { outcome = 'blocked:' + (e.code || e.message); }
      mc.log(outcome);
    `));
    assert.match(escaped.logs[0], /^(blocked:ERR_ACCESS_DENIED|no-fs)/);
    assert.doesNotMatch(escaped.logs[0], /SECRET/);
  } finally {
    fs.rmSync(canary, { force: true });
  }
});

test('an escaped program cannot open a network connection', async () => {
  let connections = 0;
  const server = net.createServer((socket) => { connections += 1; socket.end('open'); });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  try {
    // Neutralise the child's exit so the event loop would get to run the
    // connect after the result is sent.
    const escaped = await run(escapeToHostRealm(`
      process.exit = () => {};
      process.getBuiltinModule('net').connect(${port}, '127.0.0.1').on('error', () => {});
      mc.log(String(process.permission.has('net')));
    `));
    assert.deepEqual(escaped.logs, ['false']);
    await new Promise((resolve) => setTimeout(resolve, 300));
    assert.equal(connections, 0);
  } finally {
    server.close();
  }
});

test('an escaped program cannot spawn a child process', async () => {
  const escaped = await run(escapeToHostRealm(`
    let outcome = 'no-cp';
    try { outcome = process.getBuiltinModule('child_process').execSync('id').toString(); }
    catch (e) { outcome = 'blocked:' + (e.code || e.message); }
    mc.log(outcome);
  `));
  assert.match(escaped.logs[0], /^blocked:ERR_ACCESS_DENIED$/);
});

test('every known vector either stays in the vm realm or lands in the powerless process', async () => {
  const canary = path.join(os.tmpdir(), `mcbench-vector-canary-${process.pid}.txt`);
  fs.writeFileSync(canary, 'TOP-SECRET-REFERENCE-BUILD');
  // Given anything a vector leaks, climb to its realm's `process` and try to
  // read the canary. In-realm objects stop at a ReferenceError; host-realm
  // objects reach PID 1 of the build namespace and are denied the read.
  const probe = `function probe(label, leaked) {
    try {
      const P = (typeof leaked === 'function' ? leaked : leaked.constructor).constructor('return process')();
      let read;
      try { read = P.getBuiltinModule('fs').readFileSync(${JSON.stringify(canary)}, 'utf8'); } catch (e) { read = e.code; }
      mc.log(label, 'pid=' + P.pid, read);
    } catch (e) { mc.log(label, 'realm:' + e.name); }
  }`;
  const cases = {
    'custom map callback': 'const a = [0,0,0]; a.map = function (cb) { probe(label, cb); return [0,0,0]; }; mc.set(a, "stone");',
    'thrown host error': 'try { mc.get([99999,0,0]); } catch (e) { probe(label, e); }',
    'returned object': 'probe(label, mc.get([0,0,0]) || mc.spec("stone"));',
    'getter on argument': 'const a = [0,0,0]; Object.defineProperty(a, 1, { get() { probe(label, arguments.callee || this); return 0; } }); mc.set(a, "stone");',
    'Proxy argument': 'mc.set(new Proxy([0,0,0], { get(t, k, r) { if (k === "map") probe(label, r); return Reflect.get(t, k, r); } }), "stone");',
    'Symbol.species': 'class A extends Array { static get [Symbol.species]() { return function S(n) { probe(label, new.target); return []; }; } } mc.set(A.from([0,0,0]), "stone");',
    'prepareStackTrace frames': 'Error.prepareStackTrace = (e, frames) => { for (const f of frames) { const fn = f.getFunction(); if (fn) { probe(label, fn); break; } } return ""; }; try { mc.set([0.5,0,0], "stone"); } catch (e) { e.stack; } mc.log(label, "done");',
    'toJSON during mc.log': 'mc.log({ toJSON() { probe(label, this); return 1; } });',
    'async program': 'return Promise.resolve();',
    'floating promise and timers': 'Promise.resolve().then(() => { for (;;) {} }); mc.log(label, typeof setTimeout, typeof setImmediate, typeof queueMicrotask, typeof require);',
  };
  try {
    for (const [label, body] of Object.entries(cases)) {
      const src = `module.exports = (mc) => { const label = ${JSON.stringify(label)}; ${probe} ${body} };`;
      const result = await run(src).catch((e) => ({ error: formatProgramError(e), logs: e.logs || [] }));
      const text = [result.error || '', ...result.logs].join('\n');
      assert.doesNotMatch(text, /SECRET/, label);
      for (const line of result.logs.filter((l) => l.includes('pid='))) assert.match(line, new RegExp(`pid=${PID} ERR_ACCESS_DENIED$`), `${label}: ${line}`);
      if (label === 'custom map callback') assert.match(text, new RegExp(`pid=${PID} ERR_ACCESS_DENIED`), 'the confirmed vector really escapes the vm');
      if (label === 'async program') assert.match(text, /synchronous/);
      if (label === 'floating promise and timers') assert.deepEqual(result.logs, [`${label} undefined undefined undefined undefined`]);
    }
  } finally {
    fs.rmSync(canary, { force: true });
  }
});

// Runs the escape probe under the build launcher, with `nodeArgs` in place of
// the build's Node arguments, against host files made in a new folder under
// `root`: a canary, a SQLite copy of it, and an app data folder holding
// settings with an API key and another chat's world. It also tries to list
// ~/.ssh and /etc, to write into / and /dev/shm, and to read and list the
// `extra` paths.
async function runEscapeProbe(root, nodeArgs, extra = { read: [], list: [] }) {
  const { COMMAND, ARGS, ENV, NODE_ARGS } = require('../server/build-sandbox')._internal;
  const { spawn } = require('node:child_process');
  const dir = fs.mkdtempSync(path.join(root, 'mcbench-escape-'));
  const canary = path.join(dir, 'reference-build.json');
  const writeTarget = path.join(dir, 'written.txt');
  const data = path.join(dir, 'data');
  fs.writeFileSync(canary, 'TOP-SECRET-REFERENCE-BUILD');
  const db = new (require('node:sqlite').DatabaseSync)(`${canary}.db`);
  db.exec("create table secret (v text); insert into secret values ('TOP-SECRET-REFERENCE-BUILD')");
  db.close();
  fs.mkdirSync(path.join(data, 'worlds', 'other-chat'), { recursive: true });
  fs.writeFileSync(path.join(data, 'settings.json'), '{"openrouterApiKey":"TOP-SECRET-API-KEY"}');
  fs.writeFileSync(path.join(data, 'worlds', 'other-chat', 'world.json'), 'TOP-SECRET-OTHER-CHAT');
  const secrets = {
    read: [path.join(data, 'settings.json'), path.join(data, 'worlds', 'other-chat', 'world.json')],
    list: [path.join(os.homedir(), '.ssh'), path.join(data, 'worlds'), dir],
  };
  let connections = 0;
  const server = net.createServer((socket) => { connections += 1; socket.end(); });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const probe = require('./fixtures/build-escape-probe');
    const paths = { read: [...secrets.read, ...extra.read], list: [...secrets.list, '/etc', ...extra.list], write: ['/mcbench-escape.txt', '/dev/shm/mcbench-escape.txt'] };
    const call = `(${probe})(${[canary, writeTarget, server.address().port, process.pid, paths].map((v) => JSON.stringify(v)).join(', ')})`;
    const child = spawn(COMMAND, [...ARGS.slice(0, ARGS.length - NODE_ARGS.length), ...nodeArgs, '-e', call], {
      stdio: ['ignore', 'pipe', 'pipe'], env: ENV,
    });
    let out = '';
    child.stdout.on('data', (d) => { out += d; });
    await new Promise((resolve) => child.on('close', resolve));
    const secretNames = [...secrets.read.map((file) => `read ${file}`), ...secrets.list.map((folder) => `list ${folder}`)];
    const writeNames = paths.write.map((file) => `write ${file}`);
    return { results: JSON.parse(out), out, secretNames, writeNames, wrote: fs.existsSync(writeTarget), connections };
  } finally {
    server.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('code with the full build-process realm and event loop reaches nothing on the host', async () => {
  const { NODE_ARGS } = require('../server/build-sandbox')._internal;
  const { results, out, secretNames, writeNames, wrote, connections } = await runEscapeProbe(os.tmpdir(), NODE_ARGS.slice(0, -1));
  for (const name of ['readCanary', 'readPasswd', 'readProcEnviron', 'listRoot', 'list /etc', 'writeFile', 'spawn', 'worker', 'binding', 'inspector', ...secretNames, ...writeNames]) {
    assert.match(results[name], /^denied:ERR_ACCESS_DENIED$/, `${name}: ${results[name]}`);
  }
  // require stats the file before the permission check; on Linux it is not in the process's root.
  assert.match(results.requireOutside, LINUX ? /^denied:MODULE_NOT_FOUND$/ : /^denied:ERR_ACCESS_DENIED$/);
  // Node's permission model refuses the network itself; on macOS the
  // seatbelt may refuse a socket first (EPERM) or starve the resolver (ENOTFOUND).
  for (const name of ['tcpLoopback', 'tcpInternet', 'dns', 'fetch', 'listen']) {
    assert.match(results[name], LINUX ? /^denied:ERR_ACCESS_DENIED$/ : /^denied:(ERR_ACCESS_DENIED|EPERM|ENOTFOUND)$/, `${name}: ${results[name]}`);
  }
  assert.match(results.addon, /^denied:ERR_DLOPEN_DISABLED$/);
  assert.equal(results.sqlite, 'denied:unavailable');
  // Linux: the parent is outside the PID namespace; macOS: the seatbelt refuses the signal.
  assert.match(results.signalParent, LINUX ? /^denied:ESRCH$/ : /^denied:(ESRCH|EPERM)$/);
  assert.match(results.pid, new RegExp(`^allowed:${PID}$`));
  assert.equal(results.env, 'allowed:empty');
  assert.equal(wrote, false);
  assert.equal(connections, 0);
  assert.doesNotMatch(out, /SECRET|root:x:0/);
});

// The OS layer confines reads by itself: the probe under the Linux launcher
// without Node's permission model (as if it had been bypassed) finds none of
// the host's files, even ones in the same checkout as the build sources or
// in their own folder, no network, and nowhere to write; the sources stay
// readable.
test('the Linux mount namespace alone hides every host file but the build sources', { skip: !LINUX }, async () => {
  const { NODE_ARGS } = require('../server/build-sandbox')._internal;
  const unpermitted = NODE_ARGS.slice(0, -1).filter((arg) => arg !== '--permission' && !arg.startsWith('--allow-fs-read='));
  const server = path.join(__dirname, '..', 'server');
  const { results, out, secretNames, writeNames, wrote, connections } = await runEscapeProbe(__dirname, unpermitted, {
    read: [path.join(server, 'world.js'), path.join(server, 'store.js')],
    list: [server],
  });
  for (const name of ['readCanary', 'readPasswd', 'writeFile', `read ${path.join(server, 'store.js')}`, ...secretNames]) {
    assert.match(results[name], /^denied:ENOENT$/, `${name}: ${results[name]}`);
  }
  for (const name of writeNames) assert.equal(results[name], 'denied:EROFS', name);
  assert.match(results[`read ${path.join(server, 'world.js')}`], /^allowed:'use strict';/);
  assert.equal(results[`list ${server}`], 'allowed:build-child.js,build-transfer.js,mc-dsl.js,minecraft-dimension.js,world.js');
  assert.match(results['list /etc'], /^allowed:(ld\.so\.cache,?)?(localtime)?$/);
  // / holds the system folders and the first folders of the Node binary's and the sources' paths.
  const allowedRoot = new Set(['bin', 'dev', 'etc', 'lib', 'lib64', 'proc', 'sbin', 'usr', process.execPath.split('/')[1], server.split('/')[1]]);
  for (const entry of results.listRoot.replace(/^allowed:/, '').split(',')) assert.ok(allowedRoot.has(entry), `/${entry} is visible`);
  for (const name of ['tcpLoopback', 'tcpInternet', 'dns', 'fetch']) assert.match(results[name], /^denied:/, `${name}: ${results[name]}`);
  assert.equal(results.signalParent, 'denied:ESRCH');
  assert.equal(wrote, false);
  assert.equal(connections, 0);
  assert.doesNotMatch(out, /SECRET|root:x:0/);
});

test('a forged result from a compromised build process is rejected before touching the world', () => {
  const { encodeChanges, applyChanges } = require('../server/build-transfer');
  const { createBlockNormalizer } = require('../server/mc-dsl');
  const normalize = createBlockNormalizer(REGISTRY);
  const base = new World();
  base.set(0, 0, 0, 'minecraft:stone');
  base.finalize();

  // A well-formed change set, then variants a malicious child could send.
  const legit = base.clone();
  legit.set(1, 0, 0, 'minecraft:oak_planks');
  const good = encodeChanges(legit, base.palette.entries.length);
  const accepted = base.clone();
  applyChanges(accepted, good, base.palette.entries.length, normalize);
  assert.equal(accepted.get(1, 0, 0).id, 'minecraft:oak_planks');

  const paletteLength = base.palette.entries.length;
  const header = (entries) => {
    const json = Buffer.from(JSON.stringify(entries));
    const len = Buffer.alloc(4); len.writeUInt32LE(json.length);
    return Buffer.concat([len, json]);
  };
  const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
  const i32 = (n) => { const b = Buffer.alloc(4); b.writeInt32LE(n); return b; };
  const sparse = (key, pairs) => Buffer.concat([i32(key), Buffer.from([0]), (() => {
    const b = Buffer.alloc(2 + pairs.length * 4); b.writeUInt16LE(pairs.length);
    pairs.forEach(([i, v], j) => { b.writeUInt16LE(i, 2 + j * 4); b.writeUInt16LE(v, 4 + j * 4); });
    return b;
  })()]);
  const key = World.keyOf(0, 0, 0);
  const order = (keys) => Buffer.concat([u32(keys.length), ...keys.map(i32)]);
  const forged = {
    'command injection in a block id': Buffer.concat([header([['minecraft:stone run op @a', {}]]), u32(1), sparse(key, [[1, paletteLength]]), order([key])]),
    'unknown block state': Buffer.concat([header([['minecraft:oak_stairs', { facing: 'up' }]]), u32(1), sparse(key, [[1, paletteLength]]), order([key])]),
    'palette index past the palette': Buffer.concat([header([]), u32(1), sparse(key, [[1, 999]]), order([key])]),
    'cell index past the chunk': Buffer.concat([header([]), u32(1), sparse(key, [[5000, 1]]), order([key])]),
    'chunk outside the coordinate range': Buffer.concat([header([]), u32(1), sparse(World.keyOf(300, 0, 0), [[0, 1]]), order([key, World.keyOf(300, 0, 0)])]),
    'chunk dropped from the order': Buffer.concat([header([]), u32(0), order([])]),
    'truncated buffer': good.subarray(0, good.length - 3),
    'trailing bytes': Buffer.concat([good, Buffer.from([0])]),
  };
  for (const [name, buffer] of Object.entries(forged)) {
    const target = base.clone();
    assert.throws(() => applyChanges(target, buffer, paletteLength, normalize), undefined, name);
    assert.equal(target.get(0, 0, 0).id, 'minecraft:stone', name);
    assert.equal(target.size(), 1, name);
  }
});

test('a forged result past the height limit or naming new block states leaves the shared palette and chunks as they were', () => {
  const { applyChanges } = require('../server/build-transfer');
  const { createBlockNormalizer } = require('../server/mc-dsl');
  const normalize = createBlockNormalizer(REGISTRY);
  const base = new World();
  base.set(0, 0, 0, 'minecraft:stone');
  base.finalize();
  const paletteLength = base.palette.entries.length;
  const json = Buffer.from(JSON.stringify([['minecraft:oak_planks', {}]]));
  const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
  const i32 = (n) => { const b = Buffer.alloc(4); b.writeInt32LE(n); return b; };
  // One new palette entry, placed in a chunk far above Minecraft's height.
  const high = World.keyOf(0, 200, 0);
  const cells = Buffer.alloc(6); cells.writeUInt16LE(1); cells.writeUInt16LE(0, 2); cells.writeUInt16LE(paletteLength, 4);
  const buffer = Buffer.concat([u32(json.length), json, u32(1), i32(high), Buffer.from([0]), cells, u32(2), i32(World.keyOf(0, 0, 0)), i32(high)]);
  const target = base.clone();
  const chunks = target.chunks;
  assert.throws(() => applyChanges(target, buffer, paletteLength, normalize), /reaches Minecraft y=/);
  assert.equal(base.palette.entries.length, paletteLength, 'nothing was interned into the shared palette');
  assert.equal(target.chunks, chunks);
  assert.equal(target.size(), 1);
});

test('an infinite loop is stopped at the script timeout, not the wall clock', async () => {
  const started = Date.now();
  await assert.rejects(run('module.exports = mc => { while (true) {} }'), /timed out/i);
  assert.ok(Date.now() - started < WALL_TIMEOUT_MS, 'the vm timeout fired before the wall-clock kill');
});

test('a memory bomb is reported as a program error and the server survives', async () => {
  const before = process.memoryUsage().rss;
  await assert.rejects(
    run('module.exports = mc => { const a = []; for (;;) { a.push(new Array(1_000_000).fill(a.length)); } }'),
    /out of memory|timed out|exceeded/i,
  );
  // The bomb ran in the child; this process barely moved.
  assert.ok(process.memoryUsage().rss - before < 512 * 1024 * 1024);
  // The runtime is still usable afterwards.
  assert.equal((await run('module.exports = mc => mc.set([0,0,0], "stone")')).touched, 1);
});

test('a huge off-heap allocation fails at the data limit instead of filling host memory', async (t) => {
  if (!LINUX) return t.skip('RLIMIT_DATA is Linux-only; macOS bounds builds by the heap cap and wall clock');
  const started = Date.now();
  await assert.rejects(
    run('module.exports = mc => { const b = new Uint8Array(16 * 1024 ** 3); b.fill(1); mc.log(b.length); }'),
    /Array buffer allocation failed/,
  );
  assert.ok(Date.now() - started < 2000);
});

test('mc.settled crosses to the process once, on the first call, and matches the in-process read', async () => {
  const settled = new World();
  settled.fill([0, 0, 0], [40, 2, 40], 'minecraft:stone');
  settled.set(3, 3, 3, 'minecraft:oak_stairs', { facing: 'west', half: 'top' });
  const src = `module.exports = (mc) => {
    mc.log(mc.settled(3, 3, 3), mc.settled(3, 4, 3), mc.settled(40, 2, 40).id);
    mc.translate(0, 3, 0, () => mc.log(mc.settled(3, 0, 3).states.facing));
    mc.log(mc.settledTicks);
  };`;
  const expected = runInProcess(src, new World(), REGISTRY, null, { world: () => settled, ticks: 210 });
  const actual = await runProgram(src, new World(), REGISTRY, null, { world: settled, ticks: 210 });
  assert.deepEqual(actual, expected);
  assert.deepEqual(actual.logs, ['{"id":"minecraft:oak_stairs","states":{"facing":"west","half":"top"}} null minecraft:stone', 'west', '210']);
  await assert.rejects(runProgram('module.exports = mc => mc.settled(0, 0, 0)', new World(), REGISTRY, null), /no Minecraft settle for this program/);
  assert.deepEqual((await runProgram('module.exports = mc => mc.log(mc.settledTicks)', new World(), REGISTRY, null)).logs, ['null']);
});

// The OS layer confines reads by itself: a process under the build profile
// without Node's permission model (as if it had been bypassed) still cannot
// read a file outside the sources, while the sources stay readable.
test('the macOS profile alone refuses reads outside the build sources', { skip: process.platform !== 'darwin' }, () => {
  const { _internal: { ARGS } } = require('../server/build-sandbox');
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-seatbelt-')));
  const canary = path.join(dir, 'settings.json');
  const probe = path.join(dir, 'probe.js');
  fs.writeFileSync(canary, '{"apiKey":"TOP-SECRET"}');
  fs.writeFileSync(probe, `const fs = require('fs');
    const read = (file) => { try { return fs.readFileSync(file, 'utf8').slice(0, 13); } catch (error) { return error.code; } };
    console.log(JSON.stringify({ canary: read(${JSON.stringify(canary)}), source: read(${JSON.stringify(path.join(__dirname, '..', 'server', 'world.js'))}) }));`);
  try {
    // The probe itself must be readable; nothing else is added.
    const profile = `${ARGS[1]}\n(allow file-read* (literal ${JSON.stringify(probe)}))`;
    const out = require('node:child_process').execFileSync('/usr/bin/sandbox-exec', ['-p', profile, process.execPath, probe], { cwd: '/', encoding: 'utf8' });
    assert.deepEqual(JSON.parse(out), { canary: 'EPERM', source: "'use strict';" });
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
