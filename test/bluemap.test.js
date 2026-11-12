'use strict';

const assert = require('node:assert');
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { PassThrough } = require('stream');
const { test } = require('node:test');

const { BlueMap, cameraHash, renderMask } = require('../server/bluemap');

function temporaryRoot(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-lite-bluemap-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('camera links frame the active voxel build at its Minecraft height', () => {
  assert.strictEqual(
    cameraHash({ min: [-10, 2, 4], max: [9, 21, 23] }, 64),
    '#live:0:76:14:50:0:0.47:0:0:perspective',
  );
});

test('render masks leave growth room and have no height limit', () => {
  const initial = renderMask({ min: [0, 0, 0], max: [0, 0, 0] });
  assert.deepStrictEqual(initial, { minX: -64, maxX: 63, minZ: -64, maxZ: 63 });
  assert.deepStrictEqual(renderMask({ min: [0, -100, 0], max: [0, 800, 0] }), initial, 'height never changes the mask');
  assert.deepStrictEqual(renderMask({ min: [0, 0, 0], max: [31, 15, 31] }), initial, 'a small edit keeps the mask');
  assert.notDeepStrictEqual(renderMask({ min: [0, 0, 0], max: [64, 16, 64] }), initial);
});

// A BlueMap folder whose jar is already in place, and a bake that writes
// what McbenchBake writes.
function bakery(t) {
  const root = temporaryRoot(t);
  const jar = Buffer.from('bluemap');
  const runs = [];
  const blueMap = new BlueMap({
    root,
    download: async () => { throw new Error('the jar is in place'); },
    run: async (command, args, options) => {
      runs.push({ command, args, cwd: options.cwd });
      if (args.includes('McbenchBake')) {
        fs.mkdirSync(path.join(options.cwd, 'web', 'maps', 'live'), { recursive: true });
        fs.writeFileSync(path.join(options.cwd, 'web', 'index.html'), '<html>');
        fs.writeFileSync(path.join(options.cwd, 'web', 'mcbench-pack.bin'), 'pack');
        fs.writeFileSync(path.join(options.cwd, 'web', 'maps', 'live', 'settings.json'), '{}');
      }
    },
  });
  blueMap.ensureJar = async () => { fs.writeFileSync(blueMap.jar, jar); };
  const table = path.join(root, '..', `${path.basename(root)}-table.tsv`);
  fs.writeFileSync(table, 'default\tminecraft:stone\tminecraft:stone\n');
  t.after(() => fs.rmSync(table, { force: true }));
  return { root, blueMap, runs, table };
}

function request(blueMap, pathname, headers = {}) {
  const res = new PassThrough();
  res.writeHead = function (status, sent) { this.statusCode = status; this.headers = sent; return this; };
  const chunks = [];
  res.on('data', (chunk) => chunks.push(chunk));
  const finished = new Promise((resolve) => res.once('finish', resolve));
  const handled = blueMap.serve({ method: 'GET', headers }, res, pathname);
  if (!handled) return { handled };
  return finished.then(() => ({ handled, status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
}

test('the viewer\'s assets are baked once per content, and older bakes and app files are removed', async (t) => {
  const { root, blueMap, runs, table } = bakery(t);
  for (const old of ['aot', 'web', 'config', 'mcbench-renderer-c057c2cbe164.jar', '0123456789abcdef']) fs.mkdirSync(path.join(root, old), { recursive: true });
  fs.mkdirSync(path.join(root, 'data'), { recursive: true });
  const assets = await blueMap.prepare(table);
  assert.match(assets.id, /^[0-9a-f]{16}$/);
  assert.deepStrictEqual(assets, {
    id: assets.id,
    viewerUrl: `/bluemap/${assets.id}/`,
    pack: `/bluemap/${assets.id}/mcbench-pack.bin`,
    wasm: `/bluemap/mesher-${blueMap.mesherHash}.wasm`,
  });
  assert.deepStrictEqual(fs.readdirSync(root).sort(), [assets.id, 'bluemap-5.22-cli.jar', 'data'].sort());
  assert.deepStrictEqual(runs[0].args.slice(-2), ['McbenchBake.java', 'McbenchEntityAssets.java'].map((name) => path.join(__dirname, '..', 'server', name)));
  assert.strictEqual(runs[1].args.at(-1), table);
  assert.strictEqual(runs.length, 2);
  const config = path.join(root, assets.id, 'config');
  assert.match(fs.readFileSync(path.join(config, 'webapp.conf'), 'utf8'), /bluemap-bridge\.js/);
  assert.match(fs.readFileSync(path.join(config, 'maps', 'live.conf'), 'utf8'), /enable-hires: true/);
  assert.match(fs.readFileSync(path.join(config, 'core.conf'), 'utf8'), new RegExp(`data: "${path.join(root, 'data')}"`), 'downloads are shared by every bake');
  assert.strictEqual(fs.existsSync(path.join(root, assets.id, 'classes')), false);

  // A second app run finds the bake.
  const again = new BlueMap({ root, run: async () => { throw new Error('baked twice'); } });
  again.ensureJar = async () => {};
  assert.deepStrictEqual(await again.prepare(table), assets);
  // Another block table is another bake.
  fs.appendFileSync(table, 'minecraft:stone\t0\t15\t0\t1\t63\t0\n');
  const third = new BlueMap({ root, run: blueMap.run });
  third.ensureJar = async () => {};
  const other = await third.prepare(table);
  assert.notStrictEqual(other.id, assets.id);
  assert.strictEqual(fs.existsSync(path.join(root, assets.id)), false);
});

test('a failed bake leaves nothing behind and is tried again', async (t) => {
  const { root, blueMap, table } = bakery(t);
  const run = blueMap.run;
  blueMap.run = async () => { throw new Error('javac failed'); };
  await assert.rejects(blueMap.prepare(table), /javac failed/);
  assert.strictEqual(blueMap.assets, null);
  blueMap.run = run;
  const assets = await blueMap.prepare(table);
  assert.deepStrictEqual(fs.readdirSync(root).sort(), [assets.id, 'bluemap-5.22-cli.jar'].sort(), 'the partial bake is gone');
});

test('the web app, pack and mesher are served at addresses that never change meaning', async (t) => {
  const { blueMap, table } = bakery(t);
  assert.strictEqual((await request(blueMap, '/bluemap/0123456789abcdef/index.html')).status, 404, 'nothing is served before a bake');
  const assets = await blueMap.prepare(table);
  const page = await request(blueMap, assets.viewerUrl);
  assert.strictEqual(page.status, 200);
  assert.strictEqual(page.headers['content-type'], 'text/html; charset=utf-8');
  assert.strictEqual(page.headers['cache-control'], 'public, max-age=31536000, immutable');
  assert.strictEqual(String((await request(blueMap, assets.pack)).body), 'pack');
  const mesher = await request(blueMap, assets.wasm);
  assert.strictEqual(mesher.headers['content-type'], 'application/wasm');
  assert.strictEqual(crypto.createHash('sha256').update(mesher.body).digest('hex').slice(0, 16), blueMap.mesherHash);
  assert.strictEqual((await request(blueMap, `${assets.viewerUrl}maps/live/live/markers.json`)).status, 204, 'the map has no live markers');
  assert.strictEqual((await request(blueMap, `${assets.viewerUrl}../${assets.id}-other/x`)).status, 404);
  assert.strictEqual((await request(blueMap, `${assets.viewerUrl}%2e%2e/%2e%2e/bluemap-5.22-cli.jar`)).status, 404, 'nothing outside the web app');
  assert.strictEqual((await request(blueMap, '/bluemap/mesher-0000000000000000.wasm')).status, 404);
  const revalidated = await request(blueMap, assets.pack, { 'if-none-match': (await request(blueMap, assets.pack)).headers.etag });
  assert.strictEqual(revalidated.status, 304);
  assert.strictEqual((await request(blueMap, '/app.js')).handled, false);
});

test('the world controls live in the native BlueMap toolbar', () => {
  const bridge = fs.readFileSync(path.join(__dirname, '..', 'web', 'bluemap-bridge.js'), 'utf8');
  const app = fs.readFileSync(path.join(__dirname, '..', 'web', 'app.js'), 'utf8');
  assert.match(bridge, /querySelector\('\.control-bar'\)/);
  assert.match(bridge, /panel\.id = 'mcbench-panel'/);
  assert.match(bridge, /panel\.appendChild\(exportWorld\)/);
  assert.doesNotMatch(bridge, /mcbench-address/);
  assert.doesNotMatch(bridge, /mcbench-stats/);
  assert.doesNotMatch(bridge, /mcbench-lite:start-server/);
  assert.doesNotMatch(app, /class="minecraft-control/);
  assert.doesNotMatch(app, /<div class="stats">/);
});
