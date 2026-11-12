'use strict';

const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-storage-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_CHAT_MAX_BYTES = String(4 * 1024 * 1024);
const storage = require('../server/storage');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

const MiB = 1024 * 1024;
function writeFile(relative, bytes) {
  const file = path.join(dataDir, relative);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.alloc(bytes, 1));
}

test('a chat holds what du counts in its four folders, plus what was reserved since', async () => {
  const id = 'measured-chat';
  writeFile(`sandbox/${id}/data.bin`, MiB);
  writeFile(`images/${id}/shot.jpg`, MiB / 2);
  writeFile(`sandbox/other-chat/data.bin`, MiB);
  const first = storage.used(id);
  assert.ok(first >= 1.5 * MiB && first < 1.6 * MiB, `measured ${first}`);
  storage.reserve(id, MiB / 4);
  assert.equal(storage.used(id), first + MiB / 4);
  fs.rmSync(path.join(dataDir, `sandbox/${id}/data.bin`));
  await storage.measure(id);
  const second = storage.used(id);
  assert.ok(second >= MiB / 2 && second < 0.6 * MiB, `measured ${second}`);
});

test('a reservation made while a measurement runs stays counted', async () => {
  const id = 'racing-chat';
  writeFile(`attachments/${id}/file.bin`, MiB);
  const running = storage.measure(id);
  await new Promise((resolve) => setImmediate(resolve)); // du is running
  storage.reserve(id, 100_000);
  await running;
  const held = storage.used(id);
  assert.ok(held >= MiB + 100_000 && held < 1.1 * MiB + 100_000, `measured ${held}`);
});

test('after a measurement fails, the next write measures again and goes on', async (t) => {
  const id = 'failing-chat';
  writeFile(`worlds/${id}/chunk.json`, MiB);
  storage.used(id);
  // A du that fails, then the real one again.
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-du-'));
  fs.writeFileSync(path.join(bin, 'du'), '#!/bin/sh\necho "du: cannot read" >&2\nexit 1\n', { mode: 0o755 });
  const PATH = process.env.PATH;
  process.env.PATH = `${bin}${path.delimiter}${PATH}`;
  t.after(() => { process.env.PATH = PATH; fs.rmSync(bin, { recursive: true, force: true }); });
  await assert.rejects(storage.measure(id), /du did not measure/);
  assert.throws(() => storage.reserve(id, 10), /du did not measure/);
  process.env.PATH = PATH;
  storage.reserve(id, 10);
  const held = storage.used(id);
  assert.ok(held >= MiB + 10 && held < 1.1 * MiB, `measured ${held}`);
});

test('a refund while a measurement runs does not take off what du never saw', async () => {
  const id = 'refunded-chat';
  writeFile(`images/${id}/shot.jpg`, MiB);
  storage.reserve(id, 100_000);
  const running = storage.measure(id);
  await new Promise((resolve) => setImmediate(resolve)); // du is running
  // The 100 000 bytes were never written.
  storage.refund(id, 100_000);
  await running;
  const held = storage.used(id);
  assert.ok(held >= MiB, `measured ${held}`);
});

test('a refund after the chat was forgotten counts nothing', () => {
  const id = 'forgotten-chat';
  writeFile(`images/${id}/shot.jpg`, MiB);
  storage.reserve(id, 100_000);
  storage.forget(id);
  storage.refund(id, 100_000);
  const held = storage.used(id);
  assert.ok(held >= MiB && held < 1.1 * MiB, `measured ${held}`);
});

test('a write past the chat\'s budget is refused with 507 and nothing is counted', async () => {
  const id = 'full-chat';
  storage.reserve(id, 3 * MiB);
  assert.throws(() => storage.reserve(id, 2 * MiB), (error) => {
    assert.equal(error.statusCode, 507);
    assert.match(error.message, /This chat's storage is full: its attachments, images, world and \/work hold 3\.0 MiB of its 4\.0 MiB budget \(MCBENCH_CHAT_MAX_BYTES\) and this write needs 2\.0 MiB/);
    return true;
  });
  assert.equal(storage.used(id), 3 * MiB);
  storage.reserve(id, MiB);
  assert.equal(storage.refusal(id), null, 'exactly at the budget is within it');
  storage.refund(id, 4 * MiB);
  // /work grew past the budget between measurements (bash wrote it).
  writeFile(`sandbox/${id}/big.bin`, 5 * MiB);
  assert.equal(storage.refusal(id), null);
  await storage.measure(id);
  assert.match(storage.refusal(id), /storage is full: .* hold 5\.0 MiB of its 4\.0 MiB budget/);
  assert.throws(() => storage.reserve(id, 1), /storage is full/);
});

test('under the disk\'s reserve nothing is written, whatever the chat holds', (t) => {
  const id = 'small-chat';
  t.mock.method(fs, 'statfsSync', () => ({ bavail: 1000, bsize: 4096 }));
  storage._internal.dropReading();
  assert.throws(() => storage.reserve(id, 10), (error) => {
    assert.equal(error.statusCode, 507);
    assert.match(error.message, /The disk holding the data folder has 3\.9 MiB free and this write needs 1 KiB; mcbench-lite keeps 2\.00 GiB free \(MCBENCH_DISK_RESERVE_BYTES\) and saves nothing new until space is freed\./);
    return true;
  });
  assert.match(storage.refusal(id), /MCBENCH_DISK_RESERVE_BYTES/);
  storage.reserve(id, 10, { disk: false });
  t.mock.restoreAll();
  storage._internal.dropReading();
  assert.equal(storage.refusal(id), null);
  assert.ok(storage.freeBytes() > storage.DISK_RESERVE_BYTES);
});

test('expendable files go oldest first (then by name), never a referenced one, down to the number kept', () => {
  const ages = { a: 5, b: 1, c: 1, d: 3, e: 9 };
  const age = (name) => ages[name];
  assert.deepEqual(storage.expendable(['a', 'b', 'c', 'd', 'e'], new Set(), 2, age), ['b', 'c', 'd']);
  assert.deepEqual(storage.expendable(['e', 'd', 'c', 'b', 'a'], new Set(['b']), 2, age), ['c', 'd', 'a']);
  assert.deepEqual(storage.expendable(['a', 'b', 'c'], new Set(['a', 'b', 'c']), 1, age), []);
  assert.deepEqual(storage.expendable(['a', 'b'], new Set(), 2, age), []);
  // Keeping none needs no ages: every unreferenced name goes.
  assert.deepEqual(storage.expendable(['a', 'b', 'c'], new Set(['b']), 0), ['a', 'c']);
});

test('the limits are operator settings and refuse a value that is not an integer', () => {
  for (const [name, value] of [['MCBENCH_CHAT_MAX_BYTES', '4GB'], ['MCBENCH_DISK_RESERVE_BYTES', '-1'], ['MCBENCH_REPORTS_KEPT', '1.5']]) {
    const child = spawnSync(process.execPath, ['-e', 'require("./server/storage")'], {
      cwd: path.join(__dirname, '..'), env: { ...process.env, [name]: value }, encoding: 'utf8',
    });
    assert.notEqual(child.status, 0);
    assert.match(child.stderr, new RegExp(`${name} must be an integer of at least`));
  }
  assert.equal(storage.CHAT_MAX_BYTES, 4 * MiB);
  assert.equal(storage.DISK_RESERVE_BYTES, 2 * 1024 ** 3);
  assert.equal(storage.REPORTS_KEPT, 100);
});
