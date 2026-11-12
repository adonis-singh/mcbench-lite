'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const { EventEmitter } = require('node:events');
const { PassThrough } = require('node:stream');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

test('image previews preserve binary bytes, exclude stderr, and never share an output file', async (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-preview-'));
  const previous = process.env.MCBENCH_DATA_DIR;
  process.env.MCBENCH_DATA_DIR = root;
  t.after(() => {
    fs.rmSync(root, { recursive: true, force: true });
    if (previous === undefined) delete process.env.MCBENCH_DATA_DIR;
    else process.env.MCBENCH_DATA_DIR = previous;
    delete require.cache[require.resolve('../server/sandbox')];
  });
  const previews = {
    'first.png': Buffer.from([0x89, 0x50, 0x4e, 0x47, 0, 0xff, 0xfe, 1]),
    'second.png': Buffer.from([0x89, 0x50, 0x4e, 0x47, 0, 0x80, 0xfe, 2]),
  };
  t.mock.method(childProcess, 'spawn', (_command, args) => {
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    queueMicrotask(() => {
      if (args[0] === 'exec') {
        const png = previews[args.at(-1)];
        if (!png) {
          child.stderr.write('Pillow could not decode the image');
          child.emit('close', 1, null);
          return;
        }
        child.stdout.write(png.subarray(0, 3));
        child.stderr.write('Image conversion diagnostic\n');
        child.stdout.write(png.subarray(3));
      } else if (args[0] === 'inspect') {
        const crypto = require('node:crypto');
        const hash = crypto.createHash('sha256').update(fs.readFileSync(path.join(__dirname, '../sandbox/Dockerfile'))).digest('hex').slice(0, 12);
        child.stdout.write(JSON.stringify({image:`localhost/mcbench-sandbox:${hash}`,mounts:[{Type:'bind',Destination:'/work',Source:path.join(root,'sandbox','preview-test')}],readOnly:true}));
      } else {
        child.stdout.write('existing-container\n');
      }
      child.emit('close', 0, null);
    });
    return child;
  });
  const sandbox = require('../server/sandbox');
  const work = sandbox.workDir('preview-test');
  for (const name of [...Object.keys(previews), 'invalid.png']) fs.writeFileSync(path.join(work, name), 'source');
  const before = fs.readdirSync(work).sort();
  const results = await Promise.all(Object.keys(previews).map(name => sandbox.viewImage('preview-test', name)));
  assert.deepEqual(results, Object.values(previews));
  assert.deepEqual(await sandbox.viewImage('preview-test', 'first.png'), previews['first.png']);
  await assert.rejects(sandbox.viewImage('preview-test', 'invalid.png'), /Pillow could not decode the image/);
  assert.deepEqual(fs.readdirSync(work).sort(), before);
});
