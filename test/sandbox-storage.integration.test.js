'use strict';

// The storage limits on the bash sandbox, against Docker
// (MCBENCH_SANDBOX_INTEGRATION=1): a chat over its budget gets commands with
// file writes disabled until it deletes enough, the watchdog stops a
// background writer, the root filesystem takes no files, and reports are
// removed from inside the container.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const MiB = 1024 * 1024;

test('a chat over its storage budget can only delete in /work, and a background writer is stopped', {
  skip: process.env.MCBENCH_SANDBOX_INTEGRATION !== '1', timeout: 300_000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-storage-'));
  process.env.MCBENCH_DATA_DIR = root;
  process.env.MCBENCH_CHAT_MAX_BYTES = String(64 * MiB);
  const sandbox = require('../server/sandbox');
  const id = `storage-${process.pid}`;
  const work = (name) => path.join(sandbox.workDir(id), name);
  try {
    const layer = await sandbox.exec(id, 'touch /tmp/ok && echo tmp-ok; touch /var/tmp/no 2>&1; touch /no 2>&1; true', 20_000);
    assert.match(layer.formatted, /tmp-ok/);
    assert.match(layer.formatted, /'\/var\/tmp\/no': Read-only file system/);
    assert.match(layer.formatted, /'\/no': Read-only file system/);

    const filled = await sandbox.exec(id, 'dd if=/dev/zero of=big.bin bs=1M count=80 2>&1 | tail -1', 30_000);
    assert.equal(filled.storage, undefined, 'the command that passes the budget runs');
    assert.equal(fs.statSync(work('big.bin')).size, 80 * MiB);
    // The next command sees the measurement made after this one (a model's
    // next call comes seconds later; one sent at once sees the one before).
    await require('../server/storage').measure(id);

    const refused = await sandbox.exec(id, 'echo hello > note.txt; python3 -c "open(\'py.txt\',\'w\').write(\'x\')"; ls', 20_000);
    assert.match(refused.storage, /^This chat's storage is full: .* hold 80\.\d MiB of its 64\.0 MiB budget/);
    assert.match(refused.storage, /After this command: This chat's storage is full/);
    assert.match(refused.formatted, /File too large/);
    assert.equal(fs.statSync(work('note.txt')).size, 0);
    assert.equal(fs.statSync(work('py.txt')).size, 0);

    const freed = await sandbox.exec(id, 'rm big.bin && echo freed', 20_000);
    assert.match(freed.storage, /After this command the chat is back within its limits/);
    assert.match(freed.formatted, /freed/);
    const normal = await sandbox.exec(id, 'echo hello > note.txt && cat note.txt', 20_000);
    assert.equal(normal.storage, undefined);
    assert.match(normal.formatted, /hello/);

    // About 400 MiB in 10 MiB files, written in the background after the
    // command returns: the watchdog sees the disk's free space drop.
    await sandbox.exec(id, '(for i in $(seq 1 40); do dd if=/dev/zero of=bg$i.bin bs=1M count=10 2>/dev/null; sleep 0.1; done; touch bg-finished) > /dev/null 2>&1 &', 20_000);
    const count = () => fs.readdirSync(sandbox.workDir(id)).filter((name) => /^bg\d+\.bin$/.test(name)).length;
    // Until the writer finishes, or writes nothing for 3 s.
    for (let last = -1, still = 0; !fs.existsSync(work('bg-finished')) && still < 6;) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      const now = count();
      still = now === last ? still + 1 : 0;
      last = now;
    }
    const written = count();
    assert.equal(fs.existsSync(work('bg-finished')), false, 'the writer was stopped');
    assert.ok(written < 40, `${written} of 40 files written`);
    console.log(`[storage] the watchdog stopped the writer after ${written} of 40 files`);
    const after = await sandbox.exec(id, 'ls | wc -l', 20_000);
    assert.match(after.storage, /This chat's storage is full/);
    await sandbox.exec(id, 'rm -f bg*.bin', 20_000);

    // Reports are deleted inside the container: a link out of /work names
    // nothing there.
    const outside = path.join(root, 'outside');
    fs.mkdirSync(outside);
    fs.writeFileSync(path.join(outside, 'kept.json'), '{}');
    fs.writeFileSync(work('report.json'), '{}');
    fs.symlinkSync(outside, work('linked'));
    await sandbox.removeWorkFiles(id, ['report.json', 'linked/kept.json']);
    assert.equal(fs.existsSync(work('report.json')), false);
    assert.equal(fs.existsSync(path.join(outside, 'kept.json')), true);
  } finally {
    await sandbox.destroy(id);
    fs.rmSync(root, { recursive: true, force: true });
  }
});
