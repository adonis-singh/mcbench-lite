'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const { test } = require('node:test');
const { Worker } = require('node:worker_threads');

const { writeViewer, sharePageHtml } = require('../server/share-viewer');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');
const { MODULE } = require('../scripts/build-renderer');
const { mesherPack } = require('./helpers/mesher-pack');

const WEB = path.join(__dirname, '..', 'web');

function temporaryRoot(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-lite-share-viewer-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

// A baked BlueMap web folder as McbenchBake writes it, with stand-in bytes.
function bakedWeb(root) {
  const web = path.join(root, 'web');
  const files = {
    'index.html': '<!DOCTYPE html><script type="module" src="./assets/index-1.js"></script>',
    'assets/index-1.js': 'console.log("bluemap")',
    'assets/index-1.js.map': '{}',
    'assets/index-1.css': 'body{}',
    'lang/settings.conf': '{}',
    'lang/en.conf': '{}',
    'settings.json': JSON.stringify({ version: '5.22', useCookies: true, maps: ['live'], scripts: ['/bluemap-bridge.js'], styles: [] }),
    'maps/live/settings.json': JSON.stringify({ name: 'Live build', skyColor: [0.49, 0.67, 1, 1], voidColor: [0, 0, 0, 1] }),
    'maps/live/textures.json': '[]',
    'maps/live/live/markers.json': '{}',
    'maps/live/live/players.json': '{}',
    'mcbench-pack.bin': 'the pack',
    'mcbench-entities.json': '{}',
    'mcbench-entities.png': 'png',
    'sql.php': '<?php',
  };
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(web, name)), { recursive: true });
    fs.writeFileSync(path.join(web, name), text);
  }
  return web;
}

function listFiles(folder, relative = '') {
  return fs.readdirSync(path.join(folder, relative), { withFileTypes: true }).flatMap((entry) => {
    const name = path.posix.join(relative, entry.name);
    return entry.isDirectory() ? listFiles(folder, name) : [name];
  }).sort();
}

test('the viewer folder holds BlueMap\'s web app, the map, the gzipped pack and mesher, the app\'s mesher modules and the share bridge, and nothing else', (t) => {
  const root = temporaryRoot(t);
  const webRoot = bakedWeb(root);
  const mesher = path.join(root, 'mesher.wasm');
  fs.writeFileSync(mesher, 'the mesher');
  const site = path.join(root, 'site');
  const id = writeViewer(site, { webRoot, mesher });
  assert.match(id, /^[0-9a-f]{16}$/);
  const folder = path.join(site, 'viewer', id);
  assert.deepEqual(listFiles(folder), [
    'assets/index-1.css',
    'assets/index-1.js',
    'bluemap-tiles.mjs',
    'bluemap-view.mjs',
    'index.html',
    'lang/en.conf',
    'lang/settings.conf',
    'maps/live/live/markers.json',
    'maps/live/live/players.json',
    'maps/live/settings.json',
    'maps/live/textures.json',
    'mcbench-pack.bin.gz',
    'mesher-worker.mjs',
    'mesher.mjs',
    'mesher.wasm.gz',
    'settings.json',
    'share/bridge.js',
  ]);
  assert.deepEqual(fs.readdirSync(path.join(site, 'viewer')), [id], 'no staging folder is left');
  // The app's own code, byte for byte.
  for (const name of ['bluemap-tiles.mjs', 'bluemap-view.mjs', 'mesher-worker.mjs', 'mesher.mjs', 'share/bridge.js']) {
    assert.deepEqual(fs.readFileSync(path.join(folder, name)), fs.readFileSync(path.join(WEB, name)), name);
  }
  assert.equal(zlib.gunzipSync(fs.readFileSync(path.join(folder, 'mcbench-pack.bin.gz'))).toString(), 'the pack');
  assert.equal(zlib.gunzipSync(fs.readFileSync(path.join(folder, 'mesher.wasm.gz'))).toString(), 'the mesher');
  // BlueMap loads the share bridge at an address relative to the viewer, and
  // keeps no camera or settings in cookies between shares.
  const settings = JSON.parse(fs.readFileSync(path.join(folder, 'settings.json'), 'utf8'));
  assert.deepEqual(settings.scripts, ['share/bridge.js']);
  assert.equal(settings.useCookies, false);
  // No tile is asked for before the bridge runs: the site has no tile files.
  assert.deepEqual([settings.hiresSliderDefault, settings.lowresSliderDefault, settings.lowresSliderMin], [0, 0, 0]);
  assert.deepEqual(settings.maps, ['live']);
  // Sky, not black void, below the horizon.
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(folder, 'maps/live/settings.json'), 'utf8')),
    { name: 'Live build', skyColor: [0.49, 0.67, 1, 1], voidColor: [0.49, 0.67, 1, 1] });

  // The same content is the same folder, left as it is.
  const stamp = fs.statSync(path.join(folder, 'settings.json')).mtimeMs;
  assert.equal(writeViewer(site, { webRoot, mesher }), id);
  assert.equal(fs.statSync(path.join(folder, 'settings.json')).mtimeMs, stamp);
  // Other content is another folder.
  fs.writeFileSync(path.join(webRoot, 'mcbench-pack.bin'), 'another pack');
  const other = writeViewer(site, { webRoot, mesher });
  assert.notEqual(other, id);
  assert.deepEqual(fs.readdirSync(path.join(site, 'viewer')).sort(), [id, other].sort());
});

test('the share bridge and the modules it loads use only addresses relative to the viewer', () => {
  // GitHub Pages serves the site under /<repo>/: an address from the root misses it.
  for (const name of ['share/bridge.js', 'bluemap-tiles.mjs', 'bluemap-view.mjs', 'mesher-worker.mjs', 'mesher.mjs']) {
    const source = fs.readFileSync(path.join(WEB, name), 'utf8');
    assert.doesNotMatch(source, /(import\(|from |new Worker\(|fetch\()\s*['"`]\//, name);
  }
});

const PAGE = {
  viewerId: '0123456789abcdef',
  shareId: 'k3y',
  title: 'A <castle> & "keep"',
  description: 'Built by model-x in 12 turns\'',
  url: 'https://someone.github.io/builds/b/k3y/',
  image: 'https://someone.github.io/builds/b/k3y/preview.png',
  labels: ['model-x'],
};

// The content of the <meta> with this name or property, as a crawler that
// runs no JavaScript reads it.
function meta(html, key) {
  const found = html.match(new RegExp(`<meta (?:name|property)="${key}" content="([^"]*)">`));
  return found && found[1];
}

test('the share page carries X\'s player card and Open Graph tags in its static markup, escaped', () => {
  const html = sharePageHtml(PAGE);
  assert.match(html, /^<!doctype html>/i);
  const title = 'A &lt;castle&gt; &amp; &quot;keep&quot;';
  const description = 'Built by model-x in 12 turns&#39;';
  assert.equal(meta(html, 'twitter:card'), 'player');
  assert.equal(meta(html, 'twitter:title'), title);
  assert.equal(meta(html, 'twitter:description'), description);
  assert.equal(meta(html, 'twitter:player'), PAGE.url);
  assert.equal(meta(html, 'twitter:player:width'), '480');
  assert.equal(meta(html, 'twitter:player:height'), '480');
  assert.equal(meta(html, 'twitter:image'), PAGE.image);
  assert.equal(meta(html, 'og:title'), title);
  assert.equal(meta(html, 'og:description'), description);
  assert.equal(meta(html, 'og:image'), PAGE.image);
  assert.equal(meta(html, 'og:url'), PAGE.url);
  assert.equal(meta(html, 'og:type'), 'website');
  assert.match(html, new RegExp(`<title>${title}</title>`));
  assert.doesNotMatch(html, /<castle>/);
  // The viewer, at an address relative to the page, told where the build is.
  assert.match(html, /<iframe id="viewer" src="\.\.\/\.\.\/viewer\/0123456789abcdef\/\?build=\.\.\/\.\.\/b\/k3y\/build\.json"/);
  // A link that opens the page itself in a new tab.
  assert.match(html, /<a id="full" href="\.\/" target="_blank" rel="noopener">Open full screen<\/a>/);
  // A progress line, hidden until the bridge tells how far drawing has come.
  assert.match(html, /<div id="progress" role="progressbar" hidden>/);
  // X's sandbox has no modals and no forms.
  assert.doesNotMatch(html, /alert\(|confirm\(|prompt\(|<form/);
  // A share of one build has no strip of builds, and has the camera's
  // controls and the hint.
  assert.doesNotMatch(html, /id="builds"|class="pill"|id="prev"|id="next"/);
  assert.match(html, /<div id="view"><button type="button" data-mode="fly" aria-pressed="false"[^>]*>Fly<\/button><button type="button" data-mode="orbit" aria-pressed="false"[^>]*>Orbit<\/button><button type="button" id="reset" title="Reset view" aria-label="Reset view">/);
  assert.match(html, /<div id="hint" role="note"><\/div>/);
});

const LABELS = ['claude-opus-5-5-20251101', '<img src=x onerror=alert(1)>', '</script><script>alert(2)</script>'];

test('a share of several builds has a strip of them along the top, in order, the first pressed, every label escaped', () => {
  const html = sharePageHtml({ ...PAGE, labels: LABELS });
  const pills = [...html.matchAll(/<button type="button" class="pill" data-index="(\d+)" aria-pressed="(true|false)">([^<]*)<\/button>/g)];
  assert.deepEqual(pills.map((pill) => [pill[1], pill[2], pill[3]]), [
    ['0', 'true', 'claude-opus-5-5-20251101'],
    ['1', 'false', '&lt;img src=x onerror=alert(1)&gt;'],
    ['2', 'false', '&lt;/script&gt;&lt;script&gt;alert(2)&lt;/script&gt;'],
  ]);
  assert.match(html, /<nav id="builds" aria-label="Builds"><div><button type="button" id="prev" aria-label="Previous build">/);
  assert.match(html, /<button type="button" id="next" aria-label="Next build">/);
  // No label is written raw anywhere: not as markup and not in the script,
  // which has exactly one <script> and one </script>.
  assert.doesNotMatch(html, /<img/);
  assert.equal(html.match(/<script>/g).length, 1);
  assert.equal(html.match(/<\/script>/g).length, 1);
  assert.ok(html.indexOf('<nav id="builds"') < html.indexOf('<script>'), 'the strip is markup before the script');
  const script = html.slice(html.indexOf('<script>'), html.indexOf('</script>'));
  for (const label of LABELS) assert.ok(!script.includes(label), `${label} is not in the script`);
  // The tags crawlers read are those of a share of one build.
  const one = sharePageHtml(PAGE);
  for (const key of ['twitter:card', 'twitter:title', 'twitter:description', 'twitter:image', 'twitter:player', 'twitter:player:width', 'twitter:player:height', 'og:title', 'og:description', 'og:image', 'og:url', 'og:type', 'description']) {
    assert.equal(meta(html, key), meta(one, key), key);
  }
  assert.equal(html.match(/<title>[^<]*<\/title>/)[0], one.match(/<title>[^<]*<\/title>/)[0]);
  // The page and a share of one build differ only by the strip.
  assert.equal(html.replace(/<nav id="builds"[^\n]*<\/nav>\n/, ''), one);
});

test('the share page switches the camera\'s mode and resets the view through the bridge, and follows the mode the bridge tells', () => {
  const html = sharePageHtml({ ...PAGE, labels: LABELS });
  const script = html.slice(html.indexOf('<script>'), html.indexOf('</script>'));
  assert.match(script, /postMessage\(\{ type: 'mcbench-share:set-mode', mode: button\.dataset\.mode \}, window\.location\.origin\)/);
  assert.match(script, /postMessage\(\{ type: 'mcbench-share:reset' \}, window\.location\.origin\)/);
  assert.match(script, /'mcbench-share:mode'\) return markMode\(event\.data\.mode\)/);
  assert.match(script, /'mcbench-share:input'\) return hideHint\(\)/);
  // The hint is written as text, for the device the bridge chose the mode by.
  assert.match(script, /hint\.textContent = HINTS\[mode\]/);
  assert.match(script, /matchMedia\('\(pointer: coarse\) and \(hover: none\)'\)/);
  // The controls sit bottom left, clear of the progress label and the full-screen link.
  assert.match(html, /#view\{position:fixed;left:8px;bottom:8px;/);
  assert.match(html, /#progress>span\{position:absolute;left:136px;bottom:8px;/);
});

test('the share page asks the bridge for a build by index, and hears only of the build it asked for, from the viewer, on its origin', () => {
  const html = sharePageHtml({ ...PAGE, labels: LABELS });
  const script = html.slice(html.indexOf('<script>'), html.indexOf('</script>'));
  assert.match(script, /postMessage\(\{ type: 'mcbench-share:show', index: index \}, window\.location\.origin\)/);
  assert.match(script, /event\.source !== frame\.contentWindow \|\| event\.origin !== window\.location\.origin/);
  assert.match(script, /event\.data\.index !== wanted/);
  assert.match(script, /'the viewer did not load\.'\); \}, 60000\)/);
  // The strip scrolls itself, never the page around it.
  assert.doesNotMatch(script, /scrollIntoView/);
});

// The real worker (web/mesher-worker.mjs) in a Node thread, its fetch
// answering `files` (address -> bytes) and nothing else.
function worker(t, files) {
  const thread = new Worker(`
    const { parentPort, workerData } = require('node:worker_threads');
    globalThis.self = globalThis;
    self.postMessage = (message, transfer) => parentPort.postMessage(message, transfer);
    globalThis.fetch = async (url) => workerData[url] ? new Response(workerData[url]) : new Response(null, { status: 404 });
    parentPort.on('message', (data) => self.onmessage({ data }));
    import(${JSON.stringify(path.join(WEB, 'mesher-worker.mjs'))});
  `, { eval: true, workerData: files });
  t.after(() => thread.terminate());
  let next = 0;
  return (message) => new Promise((resolve, reject) => {
    const id = ++next;
    const listener = (data) => {
      if (data.id !== id) return;
      thread.off('message', listener);
      if (data.error) reject(new Error(data.error));
      else resolve(data);
    };
    thread.on('message', listener);
    thread.postMessage({ ...message, id });
  });
}

test('the worker reads an address ending in .gz as gzip, as it streams, and the same world comes of it', async (t) => {
  const blocks = [];
  for (let x = 0; x < 20; x += 1) for (let z = 0; z < 20; z += 1) blocks.push([x, 64 + ((x * z) % 5), z, x % 3 ? 'minecraft:stone' : 'minecraft:glass']);
  const blueprint = blocksBlueprint(blocks);
  const wasm = fs.readFileSync(MODULE);
  const pack = mesherPack();
  const mask = renderMask({ min: [0, 64, 0], max: [19, 68, 19] });
  const plain = worker(t, { 'm.wasm': wasm, 'p.bin': pack, 'b.bin': blueprint });
  const gzipped = worker(t, { 'm.wasm.gz': zlib.gzipSync(wasm), 'p.bin.gz': zlib.gzipSync(pack), 'b.bin.gz': zlib.gzipSync(blueprint) });
  const one = await plain({ type: 'load', wasm: 'm.wasm', pack: 'p.bin', blueprint: 'b.bin', mask, compare: false });
  const two = await gzipped({ type: 'load', wasm: 'm.wasm.gz', pack: 'p.bin.gz', blueprint: 'b.bin.gz', mask, compare: false });
  assert.ok(one.tiles.length > 0);
  assert.deepEqual(two.tiles, one.tiles);
  const [x, z] = one.tiles[0];
  const a = await plain({ type: 'snapshot', x, z });
  const b = await gzipped({ type: 'snapshot', x, z });
  assert.deepEqual(Buffer.from(b.bytes), Buffer.from(a.bytes), 'the same world, tile for tile');
  await assert.rejects(gzipped({ type: 'load', wasm: 'm.wasm.gz', pack: 'p.bin.gz', blueprint: 'gone.bin.gz', mask, compare: false }), /gone\.bin\.gz is unavailable \(HTTP 404\)/);
});

// The share bridge's view of a build's box (web/share/bridge.js), taken
// out of the script with the constants it reads.
function fitView() {
  const source = fs.readFileSync(path.join(WEB, 'share', 'bridge.js'), 'utf8');
  const from = source.indexOf('  var ROTATION');
  const to = source.indexOf('  // Past this angle');
  return new Function(`${source.slice(from, to)}; return { fit, placement, ROTATION, ANGLE };`)();
}

const BOXES = {
  tower: { min: [10, 70, -5], max: [12, 81, -3] },
  pad: { min: [-60, 64, -40], max: [59, 69, 39] },
  sphere: { min: [-150, 10, -150], max: [150, 310, 150] },
};

// Where the box's corners fall in the frame of a view from `rotation` and
// `angle` (-1 to 1 across, and up), as BlueMap's camera sees them.
function cornersSeen(view, bounds, rotation, angle, aspect) {
  const forward = [Math.sin(rotation) * Math.sin(angle), -Math.cos(angle), -Math.cos(rotation) * Math.sin(angle)];
  const right = [Math.cos(rotation), 0, Math.sin(rotation)];
  const up = [right[1] * forward[2] - right[2] * forward[1], right[2] * forward[0] - right[0] * forward[2], right[0] * forward[1] - right[1] * forward[0]];
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const eye = view.target.map((value, i) => value - forward[i] * view.distance);
  const tan = Math.tan(75 * Math.PI / 360);
  const seen = [];
  for (let corner = 0; corner < 8; corner += 1) {
    const point = [0, 1, 2].map((i) => (corner >> i & 1 ? bounds.max[i] + 1 : bounds.min[i]) - eye[i]);
    seen.push([dot(point, right) / dot(point, forward) / (tan * aspect), dot(point, up) / dot(point, forward) / tan]);
  }
  return { seen, forward };
}

test('the opening view sees the build\'s box from the south-east, 30 degrees down, whole and centered, at its middle height', () => {
  const { fit, ROTATION, ANGLE } = fitView();
  for (const [name, bounds] of Object.entries(BOXES)) {
    for (const aspect of [1, 390 / 844, 16 / 9]) {
      const view = fit(bounds, { fov: 75, aspect }, ROTATION, ANGLE);
      const { seen, forward } = cornersSeen(view, bounds, ROTATION, ANGLE, aspect);
      assert.ok(forward[0] < 0 && forward[2] < 0, 'looking north-west: the eye is south-east');
      assert.ok(Math.abs(Math.asin(-forward[1]) * 180 / Math.PI - 30) < 1e-9);
      assert.equal(view.target[1], (bounds.min[1] + bounds.max[1] + 1) / 2, `${name}: the target at the middle height`);
      for (const axis of [0, 1]) {
        const values = seen.map((point) => point[axis]);
        const [low, high] = [Math.min(...values), Math.max(...values)];
        assert.ok(low >= -1 && high <= 1, `${name} at ${aspect.toFixed(2)}: every corner in the frame`);
        assert.ok(Math.abs(low + high) < 1e-6, `${name} at ${aspect.toFixed(2)}: the picture centered`);
      }
      // Most of the frame along one axis or the other. On a wide screen a
      // flat build fills less: nearer, its near end would be drawn more than
      // five times the scale of its far end.
      const fill = Math.max(...[0, 1].map((axis) => Math.max(...seen.map((point) => point[axis])) - Math.min(...seen.map((point) => point[axis])))) / 2;
      assert.ok(fill > (name === 'pad' && aspect > 1 ? 0.5 : 0.8), `${name} at ${aspect.toFixed(2)}: fills ${fill.toFixed(2)} of the frame`);
    }
  }
});

test('a build shown after another is fitted from the direction the visitor looks: whole and centered at its middle height', () => {
  const { fit } = fitView();
  for (const [rotation, angle] of [[2.1, 0.2], [-3.3, 1.2], [7.5, 0.9], [0, 0.01]]) {
    for (const [name, bounds] of Object.entries(BOXES)) {
      const view = fit(bounds, { fov: 75, aspect: 1 }, rotation, angle);
      assert.equal(view.target[1], (bounds.min[1] + bounds.max[1] + 1) / 2, `${name}: the target at the middle height`);
      assert.ok(view.distance > 0 && Number.isFinite(view.distance), `${name}: a distance ahead`);
      const { seen } = cornersSeen(view, bounds, rotation, angle, 1);
      for (const axis of [0, 1]) {
        const values = seen.map((point) => point[axis]);
        const [low, high] = [Math.min(...values), Math.max(...values)];
        assert.ok(low >= -1 && high <= 1, `${name} from ${rotation}, ${angle}: every corner in the frame`);
        assert.ok(Math.abs(low + high) < 1e-6, `${name} from ${rotation}, ${angle}: the picture centered`);
      }
    }
  }
});

// The share bridge's reading of ?build= (web/share/bridge.js), taken out of
// the script.
function buildAddress() {
  const source = fs.readFileSync(path.join(WEB, 'share', 'bridge.js'), 'utf8');
  const from = source.indexOf('  function buildAddress');
  const to = source.indexOf('  var buildUrl');
  return new Function(`${source.slice(from, to)}; return buildAddress;`)();
}

test('the share bridge loads only a build in its own site\'s b/ folder, and throws for any other address', () => {
  const read = buildAddress();
  const viewer = 'https://someone.github.io/builds/viewer/0123456789abcdef/';
  assert.equal(read(`${viewer}?build=../../b/k3y/build.json`).href, 'https://someone.github.io/builds/b/k3y/build.json');
  assert.equal(read(`${viewer}?build=https://someone.github.io/builds/b/k3y/build.json`).href, 'https://someone.github.io/builds/b/k3y/build.json');
  for (const build of [
    'https://elsewhere.example/x.json',
    '//elsewhere.example/builds/b/x.json',
    'https://someone.github.io/other/b/x.json',
    '../../b/../x.json',
    '../../viewer/x.json',
    'data:application/json,{}',
  ]) {
    assert.throws(() => read(`${viewer}?build=${encodeURIComponent(build)}`), /is not a build of this site/, build);
  }
  assert.throws(() => read(viewer), /is not a build of this site/, 'no ?build=');
});

test('fly places the camera at the orbit view\'s eye, at distance 0, looking the same way: the same picture', () => {
  const { fit, placement, ROTATION, ANGLE } = fitView();
  for (const [rotation, angle] of [[ROTATION, ANGLE], [2.1, 0.2], [-3.3, 1.2]]) {
    for (const bounds of Object.values(BOXES)) {
      const view = fit(bounds, { fov: 75, aspect: 1 }, rotation, angle);
      const orbit = placement(view, 'orbit', rotation, angle);
      const fly = placement(view, 'fly', rotation, angle);
      assert.deepEqual(orbit, { position: view.target, distance: view.distance, rotation, angle });
      assert.deepEqual(fly, { position: view.eye, distance: 0, rotation, angle });
      // BlueMap's camera: position less distance along the view direction.
      const forward = [Math.sin(rotation) * Math.sin(angle), -Math.cos(angle), -Math.cos(rotation) * Math.sin(angle)];
      const eye = orbit.position.map((value, i) => value - forward[i] * orbit.distance);
      eye.forEach((value, i) => assert.ok(Math.abs(value - fly.position[i]) < 1e-6, 'one eye'));
    }
  }
  // Looking level, free flight still has an eye that frames the build.
  const level = fit(BOXES.tower, { fov: 75, aspect: 1 }, 0.4, Math.PI / 2);
  assert.ok(level.eye.every(Number.isFinite));
  const { seen } = cornersSeen({ target: level.eye.map((value, i) => value + [Math.sin(0.4), 0, -Math.cos(0.4)][i]), distance: 1 }, BOXES.tower, 0.4, Math.PI / 2, 1);
  for (const point of seen) assert.ok(Math.abs(point[0]) <= 1 && Math.abs(point[1]) <= 1, 'every corner in the frame');
});

test('a share opens in free flight, and in orbit on a device whose only pointer is a finger', () => {
  const source = fs.readFileSync(path.join(WEB, 'share', 'bridge.js'), 'utf8');
  const from = source.indexOf('  function openingMode');
  const to = source.indexOf('  // How far drawing');
  const openingMode = new Function(`${source.slice(from, to)}; return openingMode;`)();
  const asked = [];
  const device = (touchOnly) => (query) => { asked.push(query); return { matches: touchOnly }; };
  assert.equal(openingMode(device(false)), 'fly');
  assert.equal(openingMode(device(true)), 'orbit');
  assert.deepEqual([...new Set(asked)], ['(pointer: coarse) and (hover: none)']);
});
