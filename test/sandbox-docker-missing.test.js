'use strict';

// The sandbox with Docker faked as not installed (the CLI fails to start)
// and as installed with its daemon not running (every call fails with the
// CLI's own words): the model's bash result says which, and the idle reaper
// logs neither, while it still logs any other failure.

const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PassThrough } = require('node:stream');
const { after, mock, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-docker-missing-'));
process.env.MCBENCH_DATA_DIR = dataDir;
// storage.js keeps the real spawn for du; sandbox.js gets the fake Docker.
require('../server/storage');

// `docker` is one of: 'missing', or the stderr every call exits 1 with.
let docker = 'missing';
mock.method(childProcess, 'spawn', () => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.stdin = new PassThrough();
  child.exitCode = null;
  setTimeout(() => {
    if (docker === 'missing') {
      child.emit('error', Object.assign(new Error('spawn docker ENOENT'), { code: 'ENOENT' }));
      return;
    }
    child.stderr.write(docker);
    child.exitCode = 1;
    child.emit('close', 1, null);
  });
  return child;
});
const sandbox = require('../server/sandbox');

after(() => {
  mock.restoreAll();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

const NEW_CLI = 'failed to connect to the docker API at unix:///var/run/docker.sock; check if the path is correct and if the daemon is running: dial unix /var/run/docker.sock: connect: no such file or directory\n';
const OLD_CLI = 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?\n';

test('with Docker not installed, bash and the reaper fail with DOCKER_MISSING', async () => {
  docker = 'missing';
  await assert.rejects(sandbox.exec('no-docker', 'echo hi', 10_000), { message: sandbox.DOCKER_MISSING });
  await assert.rejects(sandbox._internal.reapIdle(), { message: sandbox.DOCKER_MISSING });
});

test('with the Docker daemon not running, bash and the reaper fail with DOCKER_NOT_RUNNING', async () => {
  for (const stderr of [NEW_CLI, OLD_CLI]) {
    docker = stderr;
    await assert.rejects(sandbox.exec('no-daemon', 'echo hi', 10_000), { message: sandbox.DOCKER_NOT_RUNNING });
    await assert.rejects(sandbox._internal.reapIdle(), { message: sandbox.DOCKER_NOT_RUNNING });
  }
});

test('the idle reaper logs nothing when Docker cannot be reached, and logs any other failure once', (t) => {
  const logged = t.mock.method(console, 'error', () => {});
  const { reaperFailed } = sandbox._internal;
  reaperFailed(new Error(sandbox.DOCKER_MISSING));
  reaperFailed(new Error(sandbox.DOCKER_NOT_RUNNING));
  assert.equal(logged.mock.callCount(), 0);
  reaperFailed(new Error('Docker sandbox ps failed (1):\nsomething else'));
  reaperFailed(new Error('Docker sandbox ps failed (1):\nsomething else'));
  assert.equal(logged.mock.callCount(), 1);
  assert.deepEqual(logged.mock.calls[0].arguments, ['[sandbox] idle reaper failed:', 'Docker sandbox ps failed (1):\nsomething else']);
});
