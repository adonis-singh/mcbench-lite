'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { pidLock } = require('../server/pid-lock');

test('a pid lock is held whole, refused while its holder lives, taken over once it is dead, and never during a takeover', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pid-lock-'));
  const lock = path.join(dir, 'run.lock');
  try {
    const release = pidLock(lock);
    assert.equal(fs.readFileSync(lock, 'utf8'), String(process.pid));
    assert.throws(() => pidLock(lock), new RegExp(`run\\.lock is held by process ${process.pid}, which is still running`));
    release();
    assert.deepEqual(fs.readdirSync(dir), []);
    // A pid no process has.
    fs.writeFileSync(lock, '2147483646');
    pidLock(lock)();
    assert.deepEqual(fs.readdirSync(dir), []);
    // Another process is taking a dead holder's lock over and does not finish: this one waits, then leaves both alone.
    fs.writeFileSync(lock, '2147483646');
    fs.writeFileSync(`${lock}.takeover`, String(process.pid));
    assert.throws(() => pidLock(lock), new RegExp(`run\\.lock\\.takeover is held by process ${process.pid}, which is still running`));
    assert.equal(fs.readFileSync(lock, 'utf8'), '2147483646');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a takeover left by a process that died in it is taken over as the lock is', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pid-lock-'));
  const lock = path.join(dir, 'run.lock');
  try {
    fs.writeFileSync(lock, '2147483646');
    fs.writeFileSync(`${lock}.takeover`, '2147483645');
    pidLock(lock)();
    assert.deepEqual(fs.readdirSync(dir), []);
    // And so is the takeover of that takeover.
    fs.writeFileSync(lock, '2147483646');
    fs.writeFileSync(`${lock}.takeover`, '2147483645');
    fs.writeFileSync(`${lock}.takeover.takeover`, '2147483644');
    const release = pidLock(lock);
    assert.deepEqual(fs.readdirSync(dir), ['run.lock']);
    assert.equal(fs.readFileSync(lock, 'utf8'), String(process.pid));
    release();
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a racer waits for a live takeover to end instead of failing', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pid-lock-'));
  const lock = path.join(dir, 'run.lock'), takeover = `${lock}.takeover`;
  // Another process that holds the takeover for 300 ms.
  const other = spawn(process.execPath, ['-e', `const fs = require('node:fs');
    fs.writeFileSync(${JSON.stringify(takeover)} + '.own', String(process.pid));
    fs.renameSync(${JSON.stringify(takeover)} + '.own', ${JSON.stringify(takeover)});
    setTimeout(() => fs.rmSync(${JSON.stringify(takeover)}), 300);`], { stdio: 'inherit' });
  try {
    fs.writeFileSync(lock, '2147483646');
    while (!fs.existsSync(takeover)) await new Promise(resolve => setTimeout(resolve, 5));
    assert.equal(fs.readFileSync(takeover, 'utf8'), String(other.pid));
    const release = pidLock(lock);
    assert.equal(fs.readFileSync(lock, 'utf8'), String(process.pid));
    release();
    assert.deepEqual(fs.readdirSync(dir), []);
  } finally {
    other.kill('SIGKILL');
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
