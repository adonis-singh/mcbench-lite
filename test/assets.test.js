'use strict';

// Minecraft's block data (server/assets.js) comes from misode/mcmeta at fixed
// commits, and each file's bytes must match the sha256 committed for it,
// downloaded or cached, before anything uses them. The cache is in the data
// folder the server runs on.
const { after, test } = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-assets-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const previousFetch = global.fetch;
after(() => {
  global.fetch = previousFetch;
  fs.rmSync(dataDir, { recursive: true, force: true });
});

const { ASSETS, MC_VERSION, getAsset, getBlockRegistry } = require('../server/assets');

const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const cached = (name) => path.join(dataDir, 'assets', MC_VERSION, name);
const fetched = [];
function serve(bytes) {
  global.fetch = async (url) => {
    fetched.push(url);
    return new Response(bytes);
  };
}

test('every file is read from a commit of misode/mcmeta, not a branch, and has a sha256', () => {
  for (const [name, { url, sha256: hash }] of Object.entries(ASSETS)) {
    assert.match(url, /^https:\/\/raw\.githubusercontent\.com\/misode\/mcmeta\/[0-9a-f]{40}\//, name);
    assert.match(hash, /^[0-9a-f]{64}$/, name);
  }
});

test('a download whose bytes are not the committed ones fails, naming both hashes, and is not cached', async () => {
  serve('{"stone":"tampered"}');
  await assert.rejects(getAsset('blocks.json'), {
    message: `blocks.json downloaded from ${ASSETS['blocks.json'].url} has sha256 ${sha256('{"stone":"tampered"}')}, but mcbench-lite expects ${ASSETS['blocks.json'].sha256}. It was not saved.`,
  });
  assert.equal(fs.existsSync(cached('blocks.json')), false);
});

test('a download with the committed bytes is cached in the data folder, and the cache is checked on every read', async () => {
  const bytes = '{"stone":[{},{}]}';
  // The real file is Mojang's data and not in the repository: the test
  // stands in bytes of its own and their hash.
  const committed = ASSETS['blocks.json'].sha256;
  ASSETS['blocks.json'].sha256 = sha256(bytes);
  try {
    serve(bytes);
    fetched.length = 0;
    assert.deepEqual(await getBlockRegistry(), { stone: [{}, {}] });
    assert.deepEqual(fetched, [ASSETS['blocks.json'].url]);
    assert.equal(fs.readFileSync(cached('blocks.json'), 'utf8'), bytes);
    assert.equal((await getAsset('blocks.json')).toString(), bytes);
    assert.equal(fetched.length, 1, 'read from the cache');
    fs.writeFileSync(cached('blocks.json'), '{"stone":"changed on disk"}');
    await assert.rejects(getAsset('blocks.json'), {
      message: `blocks.json in ${path.join(dataDir, 'assets', MC_VERSION)} has sha256 ${sha256('{"stone":"changed on disk"}')}, but mcbench-lite expects ${sha256(bytes)}. Delete it and start the app again to download it again.`,
    });
  } finally {
    ASSETS['blocks.json'].sha256 = committed;
  }
});

test('a name the table does not hold, or one inherited from Object.prototype, is unknown', async () => {
  for (const name of ['missing.json', 'constructor', '__proto__']) {
    await assert.rejects(getAsset(name), { message: `Unknown asset: ${name}` });
  }
});
