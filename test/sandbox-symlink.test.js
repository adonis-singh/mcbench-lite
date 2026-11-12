'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-symlink-'));
process.env.MCBENCH_DATA_DIR = path.join(root, 'data');
const sandbox = require('../server/sandbox');

const outside = path.join(root, 'outside');
fs.mkdirSync(outside);
const secret = path.join(outside, 'secret.txt');
fs.writeFileSync(secret, 'host secret');
const snapshot = () => Object.fromEntries(fs.readdirSync(outside).map(name => [name, fs.readFileSync(path.join(outside, name), 'utf8')]));

function workspace(id) {
  const work = sandbox.workDir(id);
  fs.writeFileSync(path.join(work, 'data.txt'), 'inside');
  fs.symlinkSync(secret, path.join(work, 'absolute'));
  fs.symlinkSync(path.relative(work, secret), path.join(work, 'relative'));
  fs.symlinkSync(outside, path.join(work, 'dir'));
  fs.symlinkSync(path.relative(work, outside), path.join(work, 'reldir'));
  fs.symlinkSync(path.join(outside, 'missing.txt'), path.join(work, 'dangling'));
  fs.symlinkSync('data.txt', path.join(work, 'inner'));
  return work;
}

test.after(() => fs.rmSync(root, { recursive: true, force: true }));

test('host reads refuse absolute, relative, directory and dangling symlinks out of /work', () => {
  workspace('reads');
  const read = sandbox.makeReadFile('reads');
  for (const name of ['absolute', 'relative', 'dir/secret.txt', 'reldir/secret.txt', 'dangling', '/work/absolute', '../outside/secret.txt'])
    assert.throws(() => read(name), /escapes \/work/, name);
  assert.equal(read('data.txt'), 'inside');
  assert.equal(read('inner'), 'inside');
  assert.throws(() => read('nothing.txt'), /No such file in \/work/);
  assert.throws(() => sandbox.resolveWorkPath('reads', 'dir/new.txt'), /escapes \/work/);
});

test('host writes never follow or replace symlinks and leave outside files untouched', () => {
  const work = workspace('writes');
  const before = snapshot();
  for (const name of ['absolute', 'relative', 'dangling', 'inner'])
    assert.throws(() => sandbox.writeWorkFile('writes', name, 'overwritten'), /symlink/, name);
  for (const name of ['dir/secret.txt', 'dir/created.txt', 'reldir/created.txt', 'dir/nested/created.txt'])
    assert.throws(() => sandbox.writeWorkFile('writes', name, 'overwritten'), /escapes \/work/, name);
  assert.deepEqual(snapshot(), before);
  assert.equal(fs.readFileSync(path.join(work, 'data.txt'), 'utf8'), 'inside');
  sandbox.writeWorkFile('writes', 'nested/out.txt', 'ok');
  assert.equal(fs.readFileSync(path.join(work, 'nested/out.txt'), 'utf8'), 'ok');
  sandbox.writeWorkFile('writes', 'data.txt', 'new');
  assert.equal(fs.readFileSync(path.join(work, 'data.txt'), 'utf8'), 'new');
});

test('an opened descriptor for a different inode cannot disclose or truncate an outside file', (t) => {
  const work = workspace('descriptor-swap');
  const target = fs.realpathSync(path.join(work, 'data.txt'));
  const originalOpen = fs.openSync;
  t.mock.method(fs, 'openSync', (file, flags, ...rest) => originalOpen(file === target ? secret : file, flags, ...rest));
  assert.throws(() => sandbox.readWorkFile('descriptor-swap', 'data.txt'), /escapes \/work/);
  assert.throws(() => sandbox.writeWorkFile('descriptor-swap', 'data.txt', 'overwritten'), /escapes \/work/);
  assert.equal(fs.readFileSync(secret, 'utf8'), 'host secret');
});

test('mirrored uploads and archived reports are not written through symlinks', () => {
  const work = workspace('mirror');
  const images = path.join(root, 'store-images', 'mirror');
  fs.mkdirSync(images, { recursive: true });
  fs.writeFileSync(path.join(images, 'shot.png'), 'png');
  fs.symlinkSync(secret, path.join(work, 'images', 'shot.png'));
  const before = snapshot();
  sandbox.syncImages('mirror', path.dirname(images));
  assert.deepEqual(snapshot(), before);
  fs.rmSync(path.join(work, 'attachments'), { recursive: true });
  fs.symlinkSync(outside, path.join(work, 'attachments'));
  const attachments = path.join(root, 'store-attachments', 'mirror');
  fs.mkdirSync(attachments, { recursive: true });
  fs.writeFileSync(path.join(attachments, 'upload.txt'), 'upload');
  assert.throws(() => sandbox.syncAttachments('mirror', path.dirname(attachments)), /escapes \/work/);
  assert.deepEqual(snapshot(), before);
  fs.symlinkSync(outside, path.join(work, 'verification'));
  assert.throws(() => sandbox.copyIntoWork('mirror', 'verification/report.json', path.join(images, 'shot.png')), /escapes \/work/);
  assert.deepEqual(snapshot(), before);
});
