'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-errors-'));
process.env.MCBENCH_DATA_DIR = dataDir;
require('./helpers/fake-engines').installFakeEngines();
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const { executeToolCall } = require('../server/agent');

after(async () => { await require('../server/shot-world').stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

// The data folder's path holds the user's name. On macOS the temporary folder
// is also reached through /private, so both spellings are looked for.
const hostPaths = [dataDir, fs.realpathSync(dataDir)];
function assertHostless(text, label) {
  for (const host of hostPaths) assert.ok(!text.includes(host), `${label} names the data folder: ${text}`);
}

// A chat whose /work has a file where the model's paths expect folders.
function chat() {
  const conversation = store.createConversation('openrouter', 'test/model');
  const work = sandbox.workDir(conversation.id);
  fs.writeFileSync(path.join(work, 'f.txt'), 'a file');
  return { conversation, work };
}

test('the host-side /work functions tell a file error by its /work path and the meaning of its code', () => {
  const { conversation: { id } } = chat();
  const cases = [
    () => sandbox.readWorkFile(id, 'f.txt/g'),
    () => sandbox.makeReadFile(id)('/work/f.txt/g'),
    () => sandbox.writeWorkFile(id, 'f.txt/g', 'x'),
    () => sandbox.writeWorkFile(id, 'f.txt/g/h', 'x'),
    () => sandbox.resolveWorkPath(id, 'f.txt/g', { write: true }),
    () => sandbox.workFileExists(id, 'f.txt/g'),
    () => sandbox.copyIntoWork(id, 'f.txt/g', __filename),
  ];
  for (const [index, run] of cases.entries()) {
    assert.throws(run, (error) => {
      assertHostless(error.message, `case ${index}`);
      assert.match(error.message, /\/work\/f\.txt\/g(\/h)?: not a directory \(ENOTDIR\)$/, `case ${index}`);
      return true;
    });
  }
  assert.throws(() => sandbox.writeWorkFile(id, 'images', 'x'), /^Error: \/work\/images: illegal operation on a directory \(EISDIR\)$/);
  // The bridge's own errors already name only the /work path.
  assert.throws(() => sandbox.readWorkFile(id, 'missing.txt'), /^Error: No such file in \/work: missing.txt$/);
});

test('a file where images/ should be is told as /work/images', () => {
  const { conversation: { id }, work } = chat();
  fs.rmSync(path.join(work, 'images'), { recursive: true });
  fs.writeFileSync(path.join(work, 'images'), 'not a folder');
  assert.throws(() => sandbox.workDir(id), /^Error: \/work\/images: file already exists \(EEXIST\)$/);
});

// Every tool that reaches /work from the host, with paths a model controls.
test('no tool result built from a file error contains the data folder path', async () => {
  const { conversation, work } = chat();
  const call = async (name, args) => (await executeToolCall(conversation, name, args, { blockRegistry: { stone: [{}, {}] }, imageInput: true })).result;
  const results = {
    readFile: await call('build', { program: 'module.exports = mc => mc.log(mc.readFile("f.txt/g"))' }),
    writeFile: await call('build', { program: 'module.exports = mc => mc.writeFile("f.txt/g", "x")' }),
    view_image: await call('view_image', { path: 'f.txt/g' }),
  };
  // test_build lists /work/verification before it runs, which a model can make a file.
  fs.writeFileSync(path.join(work, 'verification'), 'not a folder');
  results.test_build = await call('test_build', { tests: [{}] });
  // bash mirrors uploads into /work/images first, which a model can make a file.
  fs.rmSync(path.join(work, 'images'), { recursive: true });
  fs.writeFileSync(path.join(work, 'images'), 'not a folder');
  results.bash = await call('bash', { command: 'true' });
  for (const [name, result] of Object.entries(results)) {
    assert.equal(result.ok, false, name);
    assertHostless(JSON.stringify(result), name);
    assert.match(result.error, /\/work\/(f\.txt\/g|verification|images): (not a directory \(ENOTDIR\)|file already exists \(EEXIST\))/, name);
  }
});
