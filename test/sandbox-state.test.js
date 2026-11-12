'use strict';

// The sandbox with Docker faked: containers an earlier run left are adopted
// for the idle reaper and the storage watchdog.

const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PassThrough } = require('node:stream');
const { after, mock, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-state-'));
process.env.MCBENCH_DATA_DIR = dataDir;
require('../server/storage');

let listed = '';
mock.method(childProcess, 'spawn', () => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.stdin = new PassThrough();
  setTimeout(() => {
    child.stdout.write(listed);
    child.emit('close', 0, null);
  });
  return child;
});
const sandbox = require('../server/sandbox');

after(() => {
  mock.restoreAll();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

test('containers an earlier run left are adopted, and only this /work root\'s', async () => {
  const { containerName, reapIdle, sandboxes } = sandbox._internal;
  listed = [containerName('left-behind'), containerName('left-behind', { network: true }), 'mcb-v3-000000000000-other-root', 'unrelated'].join('\n');
  await reapIdle();
  assert.deepEqual([...sandboxes.keys()], ['left-behind']);
  sandboxes.clear();
});

