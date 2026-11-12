'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

test('a failed WebAssembly build is reported, and the next prepare builds again', async () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-wasm-')), 'core.wasm');
  fs.writeFileSync(`${file}.sources`, '0123456789abcdef0123');
  let attempts = 0;
  // No cargo here: the build is faked before core-wasm takes it.
  require('../scripts/build-wasm').build = async () => {
    attempts += 1;
    if (attempts === 1) throw Object.assign(new Error('cargo failed'), { stderr: 'error[E0425]: cannot find value' });
    return file;
  };
  const coreWasm = require('../server/core-wasm');
  await assert.rejects(coreWasm.prepare(), /cargo failed/);
  assert.match(coreWasm.state().error, /could not be built: error\[E0425\]/);
  const again = coreWasm.prepare();
  assert.deepEqual(coreWasm.state(), {}, 'building again');
  assert.equal(await again, await coreWasm.prepare(), 'one build while it runs');
  assert.deepEqual(coreWasm.state(), { url: '/core/0123456789abcdef.wasm' });
  assert.equal(attempts, 2);
});

// A fresh core-wasm on a faked build (no cargo here).
function fresh(build) {
  require('../scripts/build-wasm').build = build;
  delete require.cache[require.resolve('../server/core-wasm')];
  return require('../server/core-wasm');
}

test('a failed build is built again when the page asks for the engine, and not back to back', async (t) => {
  t.mock.timers.enable({ apis: ['Date'] });
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-wasm-')), 'core.wasm');
  fs.writeFileSync(`${file}.sources`, '0123456789abcdef0123');
  let attempts = 0;
  const coreWasm = fresh(async () => {
    attempts += 1;
    if (attempts === 1) throw Object.assign(new Error('cargo failed'), { stderr: 'error[E0425]: cannot find value' });
    return file;
  });
  // The server's start builds once, and nothing else calls prepare.
  await assert.rejects(coreWasm.prepare(), /cargo failed/);
  // The page asks every second. It is told of the failure, and no build
  // starts that soon after it.
  assert.match(coreWasm.state().error, /could not be built: error\[E0425\]/);
  t.mock.timers.tick(29_000);
  assert.match(coreWasm.state().error, /could not be built: error\[E0425\]/);
  assert.equal(attempts, 1);
  // Half a minute after the failure, its asking starts the next build.
  t.mock.timers.tick(1_000);
  assert.match(coreWasm.state().error, /could not be built: error\[E0425\]/);
  assert.deepEqual(coreWasm.state(), {}, 'building again');
  // The build starts once Minecraft's tables are there (a turn later).
  await new Promise(setImmediate);
  assert.equal(attempts, 2);
  await coreWasm.prepare();
  assert.deepEqual(coreWasm.state(), { url: '/core/0123456789abcdef.wasm' });
  assert.equal(attempts, 2, 'a built module is not built again');
});

test('a module whose sources stamp cannot be read has failed: it is not building for ever', async () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-wasm-')), 'core.wasm');
  let attempts = 0;
  const coreWasm = fresh(async () => { attempts += 1; return file; });
  await assert.rejects(coreWasm.prepare(), /ENOENT/);
  assert.match(coreWasm.state().error, /could not be built: ENOENT/);
  // Asked for outright, it is built again, and fails the same way.
  await assert.rejects(coreWasm.prepare(), /ENOENT/);
  assert.equal(attempts, 2);
  fs.writeFileSync(`${file}.sources`, 'fedcba98765432100000');
  assert.deepEqual(await coreWasm.prepare(), { file, url: '/core/fedcba9876543210.wasm' });
  assert.deepEqual(coreWasm.state(), { url: '/core/fedcba9876543210.wasm' });
});
