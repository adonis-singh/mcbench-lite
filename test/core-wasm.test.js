'use strict';

// The page's engine (server/core-wasm.js): the module this checkout runs and
// Minecraft's tables it reads, served from one address named by the module's
// version and the tables' hash. The engine's install and the tables folder
// are faked here.
const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const engine = require('../server/engine');
const { minecraftLive } = require('../server/minecraft-live');

const real = { installEngine: engine.installEngine, tablesFolder: engine.tablesFolder, minecraftTables: minecraftLive.minecraftTables };
test.after(() => {
  Object.assign(engine, { installEngine: real.installEngine, tablesFolder: real.tablesFolder });
  minecraftLive.minecraftTables = real.minecraftTables;
});

// A tools folder with a module of version 0123456789abcdef and a tables
// folder, and a fresh core-wasm over them whose install is `install`.
function fresh(install) {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-wasm-'));
  const module = path.join(scratch, 'engine-wasm-0123456789abcdef', 'mcbench-core.wasm');
  fs.mkdirSync(path.dirname(module));
  fs.writeFileSync(module, Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]));
  const tables = path.join(scratch, 'tables');
  fs.mkdirSync(tables);
  fs.writeFileSync(path.join(tables, 'blocks.txt'), 'block\tminecraft:air\n');
  fs.writeFileSync(path.join(tables, 'loot.json'), '{}\n');
  engine.installEngine = async () => install({ program: path.join(scratch, 'engine', 'mcbench-core'), module });
  engine.tablesFolder = () => tables;
  minecraftLive.minecraftTables = async () => {};
  delete require.cache[require.resolve('../server/core-wasm')];
  const hash = crypto.createHash('sha256');
  for (const name of ['blocks.txt', 'loot.json']) hash.update(`${name}\0`).update(fs.readFileSync(path.join(tables, name))).update('\0');
  return { coreWasm: require('../server/core-wasm'), module, tables, address: `/core/0123456789abcdef-${hash.digest('hex').slice(0, 16)}/` };
}

// A response that keeps what serve() writes.
function response() {
  return {
    writeHead(status, headers) { Object.assign(this, { status, headers }); },
    end(body) { this.body = body; },
  };
}

test('the module and its two tables are served from the address the page is told, for good, and nothing else', async () => {
  const { coreWasm, module, tables, address } = fresh((files) => files);
  await coreWasm.prepare();
  assert.deepEqual(coreWasm.state(), { url: address });
  for (const [name, file, type] of [['engine.wasm', module, 'application/wasm'], ['blocks.txt', path.join(tables, 'blocks.txt'), 'text/plain; charset=utf-8'], ['loot.json', path.join(tables, 'loot.json'), 'application/json']]) {
    const res = response();
    assert.equal(await coreWasm.serve({ method: 'GET' }, res, `${address}${name}`), true);
    assert.equal(res.status, 200, name);
    assert.equal(res.headers['content-type'], type);
    assert.equal(res.headers['cache-control'], 'private, max-age=31536000, immutable');
    assert.deepEqual(res.body, fs.readFileSync(file));
  }
  for (const pathname of [`${address}datafix.txt`, `${address}../blocks.txt`, '/core/0123456789abcdef-0000000000000000/blocks.txt']) {
    const res = response();
    assert.equal(await coreWasm.serve({ method: 'GET' }, res, pathname), true);
    assert.equal(res.status, 404, pathname);
  }
  assert.equal(await coreWasm.serve({ method: 'GET' }, response(), '/web/app.js'), false);
});

test('a failed install is reported, and the next prepare installs again', async () => {
  let attempts = 0;
  const { coreWasm, address } = fresh((files) => {
    attempts += 1;
    if (attempts === 1) throw new Error('The download of https://example/engine failed: HTTP 503');
    return files;
  });
  await assert.rejects(coreWasm.prepare(), /HTTP 503/);
  assert.match(coreWasm.state().error, /could not be prepared: The download of .* failed: HTTP 503/);
  const again = coreWasm.prepare();
  assert.deepEqual(coreWasm.state(), {}, 'preparing again');
  assert.equal(await again, await coreWasm.prepare(), 'one try while it runs');
  assert.deepEqual(coreWasm.state(), { url: address });
  assert.equal(attempts, 2);
});

test('a failure is tried again when the page asks for the engine, and not back to back', async (t) => {
  t.mock.timers.enable({ apis: ['Date'] });
  let attempts = 0;
  const { coreWasm, address } = fresh((files) => {
    attempts += 1;
    if (attempts === 1) throw new Error('core/ is not the released engine\'s sources');
    return files;
  });
  // The server's start prepares once, and nothing else calls prepare.
  await assert.rejects(coreWasm.prepare(), /not the released engine/);
  // The page asks every second. It is told of the failure, and no try
  // starts that soon after it.
  assert.match(coreWasm.state().error, /not the released engine/);
  t.mock.timers.tick(29_000);
  assert.match(coreWasm.state().error, /not the released engine/);
  assert.equal(attempts, 1);
  // Half a minute after the failure, its asking starts the next try.
  t.mock.timers.tick(1_000);
  assert.match(coreWasm.state().error, /not the released engine/);
  assert.deepEqual(coreWasm.state(), {}, 'preparing again');
  await coreWasm.prepare();
  assert.deepEqual(coreWasm.state(), { url: address });
  assert.equal(attempts, 2, 'a prepared engine is not prepared again');
});
