'use strict';

// The states of a chat's sandbox, with Docker faked: the storage watchdog
// leaves the processes of a chat alone while its write-disabled command
// runs, and a command that arrives while the chat's containers are being
// removed waits for the removal and gets a fresh container.

const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PassThrough } = require('node:stream');
const { after, mock, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-lifecycle-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_CHAT_MAX_BYTES = String(1024 * 1024);
// storage.js keeps the real spawn for du; sandbox.js gets the fake Docker.
const storage = require('../server/storage');

// The fake Docker: containers by name, each with the persistent shells open
// in it. A CLI call that `held` matches stays open until `release()`. While
// a container's `rm -f` is open the container still inspects as running, its
// shells answer nothing, and they close when the removal completes.
const containers = new Map(); // name -> { shells, dying, sysctls, networks }
const calls = [];
let held = () => false;
let release = () => {};
let released = Promise.resolve();
// A CLI call that `missing` matches fails to start, as with no Docker.
let missing = () => false;
function hold(match) {
  held = match;
  released = new Promise((resolve) => { release = () => { held = () => false; resolve(); }; });
}

mock.method(childProcess, 'spawn', (command, args) => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.stdin = new PassThrough();
  child.exitCode = null;
  child.killed = false;
  const close = (code) => {
    if (child.exitCode !== null) return;
    child.exitCode = code;
    child.emit('close', code, null);
  };
  child.kill = () => { child.killed = true; close(137); return true; };
  calls.push(args);
  if (missing(args)) { queueMicrotask(() => child.emit('error', new Error('spawn docker ENOENT'))); return child; }
  const waiting = held(args) ? released : Promise.resolve();
  const name = args.find((arg) => arg.startsWith('mcb-'));
  if (args[0] === 'exec' && args[1] === '-i') {
    const container = containers.get(name);
    if (!container) { queueMicrotask(() => close(1)); return child; }
    container.shells.add(child);
    child.stdin.on('data', (chunk) => {
      const [, script, token] = /^([\s\S]*)\nprintf '\\n(\w+) %s\\n' "\$\?"\n$/.exec(chunk.toString());
      if (!container.dying) child.stdout.write(`ran: ${script}\n\n${token} 0\n`);
    });
    return child;
  }
  if (args[0] === 'rm' && containers.has(name)) containers.get(name).dying = true;
  waiting.then(() => {
    let code = 0;
    if (args[0] === 'ps') child.stdout.write([...containers.keys()].join('\n'));
    else if (args[0] === 'image') child.stdout.write('present\n');
    else if (args[0] === 'inspect') {
      if (containers.has(name)) {
        child.stdout.write(JSON.stringify({
          image: sandbox.IMAGE, mounts: [{ Type: 'bind', Destination: '/work', Source: sandbox.workDir(name.split('-').slice(3).join('-')) }],
          running: true, networks: containers.get(name).networks, readOnly: true, sysctls: containers.get(name).sysctls,
        }));
      } else code = 1;
    } else if (args[0] === 'run' && args[1] === '-d') {
      const sysctls = Object.fromEntries(args.flatMap((arg, i) => (args[i - 1] === '--sysctl' ? [arg.split('=')] : [])));
      containers.set(name, { shells: new Set(), dying: false, sysctls: Object.keys(sysctls).length ? sysctls : null, networks: {} });
    } else if (args[0] === 'network' && args[1] === 'connect') containers.get(name).networks = { [args[2]]: {} };
    else if (args[0] === 'rm') {
      for (const shell of containers.get(name)?.shells || []) shell.kill();
      containers.delete(name);
    } else if (args[0] === 'exec' && !containers.has(name)) code = 1;
    else if (args[0] === 'exec' && args.at(-1) !== 'kill -KILL -1') child.stdout.write(`ran: ${args.at(-1)}\n`);
    close(code);
  });
  return child;
});
const sandbox = require('../server/sandbox');

after(() => {
  mock.restoreAll();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const kills = () => calls.filter((args) => args.includes('kill -KILL -1')).length;
const oneShot = (args) => args[0] === 'exec' && args.includes('mcb');

test('the watchdog does not end the processes of a chat while its write-disabled command runs', async (t) => {
  const id = 'cleanup';
  // The disk is under its reserve: every command runs with writes disabled,
  // and the watchdog measures the chats every second.
  t.mock.method(fs, 'statfsSync', () => ({ bavail: 1000, bsize: 4096 }));
  storage._internal.dropReading();
  t.after(() => { release(); storage._internal.dropReading(); });
  await storage.measure(id);
  hold(oneShot);
  const running = sandbox.exec(id, 'rm -rf scratch', 10_000);
  while (!calls.some(oneShot)) await pause(1);
  calls.length = 0;
  await sandbox._internal.watchStorage();
  assert.equal(kills(), 0, 'the command that frees space is left running');
  release();
  const result = await running;
  assert.equal(result.exitCode, 0);
  assert.match(result.formatted, /Exit code: 0[\s\S]*ran: rm -rf scratch/);
  assert.match(result.storage, /^The disk holding the data folder has 3\.9 MiB free/);
  // With no such command running, the chat's processes are ended as before.
  calls.length = 0;
  await sandbox._internal.watchStorage();
  assert.equal(kills(), 2, 'both of its containers');
});

test('a halt under way ends before the chat\'s write-disabled command starts', async (t) => {
  const id = 'halting';
  t.mock.method(fs, 'statfsSync', () => ({ bavail: 1000, bsize: 4096 }));
  storage._internal.dropReading();
  t.after(() => { release(); storage._internal.dropReading(); });
  await sandbox.exec(id, 'true', 10_000);
  // The watchdog's two kills of this chat stay open; the command's own do not.
  let watchdogs = 0;
  hold((args) => args.includes('kill -KILL -1') && args[1].endsWith(`-${id}`) && watchdogs++ < 2);
  calls.length = 0;
  const watching = sandbox._internal.watchStorage();
  while (watchdogs < 2) await pause(1);
  const running = sandbox.exec(id, 'rm -rf scratch', 10_000);
  await pause(30);
  assert.equal(calls.filter(oneShot).length, 0, 'the command waits for the kills to land');
  release();
  const result = await running;
  assert.match(result.formatted, /Exit code: 0[\s\S]*ran: rm -rf scratch/);
  await watching;
});

test('a command that arrives while the idle reaper removes the containers waits, and runs in a fresh container', async (t) => {
  const id = 'reaped';
  t.after(() => release());
  const first = await sandbox.exec(id, 'echo one', 10_000);
  assert.match(first.formatted, /Exit code: 0[\s\S]*ran: echo one/);
  assert.equal(calls.filter((args) => args[0] === 'run').length, 1);
  // Eleven minutes on, the chat is idle; its containers' removal stays open.
  const now = Date.now();
  const later = t.mock.method(Date, 'now', () => now + 11 * 60 * 1000);
  hold((args) => args[0] === 'rm');
  await sandbox._internal.reapIdle();
  later.mock.restore();
  assert.equal(calls.filter((args) => args[0] === 'rm' && args.at(-1).endsWith(`-${id}`)).length, 2, 'both containers are being removed');
  calls.length = 0;
  const second = sandbox.exec(id, 'echo two', 10_000);
  await pause(30);
  assert.deepEqual(calls, [], 'nothing is asked of a container that is being removed');
  release();
  const result = await second;
  assert.match(result.formatted, /Exit code: 0[\s\S]*ran: echo two/);
  assert.deepEqual(calls.filter((args) => args[0] !== 'image').map((args) => args.slice(0, 2).join(' ')), ['inspect -f', 'run -d', 'exec -i'], 'a fresh container and shell');
});

test('a reset that arrives during a removal waits for it, and removes nothing twice', async (t) => {
  const id = 'twice';
  t.after(() => release());
  await sandbox.exec(id, 'echo one', 10_000);
  calls.length = 0;
  hold((args) => args[0] === 'rm');
  const resets = [sandbox.reset(id), sandbox.reset(id)];
  let done = 0;
  for (const reset of resets) reset.then(() => { done += 1; });
  await pause(10);
  assert.equal(done, 0);
  release();
  await Promise.all(resets);
  assert.equal(calls.filter((args) => args[0] === 'rm').length, 2, 'one removal of each container');
  assert.equal(sandbox._internal.sandboxes?.has(id), false, 'a removed sandbox is forgotten');
});

// A global IPv6 address can be the host's own or a LAN device's, so the
// networked container has no IPv6 at all, and one made before that is made again.
test('the networked container is made with IPv6 off, and one with IPv6 on is replaced', async () => {
  const id = 'ipv6';
  const name = sandbox._internal.containerName(id, { network: true });
  const ipv6Off = { 'net.ipv6.conf.all.disable_ipv6': '1', 'net.ipv6.conf.default.disable_ipv6': '1' };
  calls.length = 0;
  await sandbox.exec(id, 'curl https://example.com', 10_000, { network: true });
  assert.deepEqual(containers.get(name).sysctls, ipv6Off);
  assert.deepEqual(containers.get(name).networks, { bridge: {} });
  assert.equal(calls.filter((args) => args[0] === 'run' && args[1] === '-d').length, 1);
  // A warm one is used as it is.
  calls.length = 0;
  await sandbox.exec(id, 'true', 10_000, { network: true });
  assert.equal(calls.filter((args) => args[0] === 'rm' || args[0] === 'run').length, 0);
  // One an earlier version made, with IPv6 on, is removed and made again.
  containers.get(name).sysctls = null;
  calls.length = 0;
  await sandbox.exec(id, 'true', 10_000, { network: true });
  assert.deepEqual(calls.filter((args) => args[0] === 'rm' || (args[0] === 'run' && args[1] === '-d')).map((args) => args[0]), ['rm', 'run']);
  assert.deepEqual(containers.get(name).sysctls, ipv6Off);
  // The persistent shell's container has no network to turn off.
  await sandbox.exec(id, 'true', 10_000);
  assert.equal(containers.get(sandbox._internal.containerName(id)).sysctls, null);
});

// Last: shutdown stops the sandbox module.
test('shutdown removes every container this process tracks, and a Docker that cannot be reached does not fail it', async () => {
  await sandbox.exec('kept', 'echo a', 10_000);
  await sandbox.exec('unreachable', 'echo b', 10_000);
  // The Docker CLI cannot start for the second chat's removal.
  missing = (args) => args[0] === 'rm' && args.at(-1).endsWith('-unreachable');
  calls.length = 0;
  await sandbox.shutdown();
  const removed = calls.filter((args) => args[0] === 'rm').map((args) => args.at(-1));
  for (const id of ['kept', 'unreachable', 'reaped']) {
    for (const network of [false, true]) assert.ok(removed.includes(sandbox._internal.containerName(id, { network })), `${id} ${network}`);
  }
  assert.equal(sandbox._internal.sandboxes.size, 0, 'nothing is tracked');
  assert.deepEqual([...containers.keys()].filter((name) => name.endsWith('-kept')), [], 'the reachable containers are gone');
});
