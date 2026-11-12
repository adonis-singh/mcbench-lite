'use strict';

// bash under the storage limits, with Docker faked: over the chat's budget a
// command runs in a one-shot shell with file writes disabled, and the
// watchdog ends the processes of a chat whose /work grew past its budget.
// test/sandbox-storage.integration.test.js runs the same against Docker.

const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const crypto = require('node:crypto');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PassThrough } = require('node:stream');
const { after, mock, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-storage-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_CHAT_MAX_BYTES = String(1024 * 1024);
// storage.js keeps the real spawn for du, and build-sandbox.js (which agent.js
// loads) for the spare build process it starts on load; sandbox.js gets the
// fake Docker.
const storage = require('../server/storage');
require('../server/build-sandbox');

const calls = [];
let onExec = () => ({ output: '', code: 0 });
mock.method(childProcess, 'spawn', (command, args) => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  calls.push(args);
  queueMicrotask(() => {
    let code = 0;
    if (args[0] === 'inspect') {
      const image = `localhost/mcbench-sandbox:${crypto.createHash('sha256').update(fs.readFileSync(path.join(__dirname, '../sandbox/Dockerfile'))).digest('hex').slice(0, 12)}`;
      const source = path.join(dataDir, 'sandbox', args.at(-1).split('-').slice(3).join('-'));
      child.stdout.write(JSON.stringify({ image, mounts: [{ Type: 'bind', Destination: '/work', Source: source }], running: true, networks: {}, readOnly: true }));
    } else if (args[0] === 'exec' && args.includes('-c') && args[args.indexOf('-c') + 1] !== 'kill -KILL -1') {
      const result = onExec(args.at(-1));
      child.stdout.write(result.output);
      code = result.code;
    }
    child.emit('close', code, null);
  });
  return child;
});
const sandbox = require('../server/sandbox');

after(() => {
  mock.restoreAll();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

const id = 'storage-chat';
const big = () => path.join(sandbox.workDir(id), 'big.bin');

test('over its budget a chat\'s bash command runs with file writes disabled, and says so', async () => {
  fs.writeFileSync(big(), Buffer.alloc(2 * 1024 * 1024));
  await storage.measure(id);
  onExec = (command) => {
    assert.equal(command, 'rm big.bin');
    fs.rmSync(big());
    return { output: 'removed\n', code: 0 };
  };
  calls.length = 0;
  const result = await sandbox.exec(id, 'rm big.bin', 10_000);
  const run = calls.find((args) => args.at(-1) === 'rm big.bin');
  assert.match(run[run.indexOf('-c') + 1], /^ulimit -f 0; trap "" XFSZ; timeout -k 2 /);
  assert.ok(calls.some((args) => args.includes('kill -KILL -1')), 'the session and its background jobs are ended first');
  assert.match(result.storage, /^This chat's storage is full: its attachments, images, world and \/work hold 2\.0 MiB of its 1\.0 MiB budget \(MCBENCH_CHAT_MAX_BYTES\)\. While that lasts, bash runs each command in a fresh shell with file writes disabled \(ulimit -f 0\)/);
  assert.match(result.storage, /After this command the chat is back within its limits; the next command runs normally\.$/);
  assert.match(result.formatted, /Exit code: 0[\s\S]*removed/);
  assert.equal(storage.refusal(id), null);
});

test('the agent returns that command as a failed bash call with its output', async () => {
  const { _internal: { executeBash } } = require('../server/agent');
  fs.writeFileSync(big(), Buffer.alloc(2 * 1024 * 1024));
  await storage.measure(id);
  onExec = () => ({ output: 'bash: line 1: echo: write error: File too large\n', code: 1 });
  const result = await executeBash({ id }, { command: 'echo more > more.txt' });
  assert.equal(result.ok, false);
  assert.match(result.error, /storage is full[\s\S]*After this command: This chat's storage is full/);
  assert.match(result.output, /File too large/);
});

test('the watchdog ends the processes of a chat whose /work passed its budget, once the disk loses space', async (t) => {
  let free = 50 * 1024 ** 3;
  t.mock.method(fs, 'statfsSync', () => ({ bavail: free / 4096, bsize: 4096 }));
  // Each tick reads the disk afresh, as ticks a second apart do.
  const tick = () => { storage._internal.dropReading(); return sandbox._internal.watchStorage(); };
  fs.rmSync(big());
  await storage.measure(id);
  await tick(); // the baseline
  fs.writeFileSync(big(), Buffer.alloc(2 * 1024 * 1024));
  calls.length = 0;
  free -= 30 * 1024 ** 2;
  await tick();
  assert.equal(calls.filter((args) => args.includes('kill -KILL -1')).length, 0, 'a small drop measures nothing');
  free -= 300 * 1024 ** 2;
  await tick();
  assert.equal(calls.filter((args) => args.includes('kill -KILL -1')).length, 2, 'both of its containers');
  calls.length = 0;
  await tick();
  free -= 300 * 1024 ** 2;
  await tick();
  assert.equal(calls.filter((args) => args.includes('kill -KILL -1')).length, 0, 'ended once until a command learns why');
});

test('under the disk\'s reserve every chat\'s command runs with writes disabled', async (t) => {
  t.mock.method(fs, 'statfsSync', () => ({ bavail: 1000, bsize: 4096 }));
  storage._internal.dropReading();
  fs.rmSync(big(), { force: true });
  await storage.measure(id);
  onExec = () => ({ output: '', code: 0 });
  const result = await sandbox.exec(id, 'ls', 10_000);
  assert.match(result.storage, /^The disk holding the data folder has 3\.9 MiB free; mcbench-lite keeps 2\.00 GiB free \(MCBENCH_DISK_RESERVE_BYTES\)/);
});
