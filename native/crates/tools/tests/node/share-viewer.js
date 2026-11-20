'use strict';

// Today's side of tests/share_viewer.rs.
//
//   node share-viewer.js diff <web folder> <out folder>
//     today's writeViewer into <out>/node-site with the bake's web folder
//     and the repository's mesher, and today's sharePageHtml of three
//     inputs into <out>/pages.json; prints the viewer id.
//   node share-viewer.js case <name> <viewer folder>
//     the test of test/share-viewer.test.js named <name> whose subject is
//     the bridge's or the worker's JavaScript, run against the files of a
//     viewer folder mcbench_tools::share_viewer wrote.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');
const { Worker } = require('node:worker_threads');

const ROOT = path.join(__dirname, '..', '..', '..', '..', '..');
const { writeViewer, sharePageHtml } = require(path.join(ROOT, 'server', 'share-viewer'));

const PAGE = {
  viewerId: '0123456789abcdef',
  shareId: 'k3y',
  title: 'A <castle> & "keep"',
  description: 'Built by model-x in 12 turns\'',
  url: 'https://someone.github.io/builds/b/k3y/',
  image: 'https://someone.github.io/builds/b/k3y/preview.png',
  labels: ['model-x'],
};
const PAGES = [
  { ...PAGE, title: 'A castle', description: 'Built by model-x in 12 turns' },
  { ...PAGE, title: 'Towers and a bridge', labels: ['claude-opus-5-5-20251101', 'gpt-6', 'gemini-4-pro'] },
  { ...PAGE, labels: ['<img src=x onerror=alert(1)>', '</script>&\'"'] },
];

function diff(webRoot, out) {
  const mesher = path.join(ROOT, 'renderer', 'mcbench-renderer.wasm');
  const id = writeViewer(path.join(out, 'node-site'), { webRoot, mesher });
  fs.writeFileSync(path.join(out, 'pages.json'), JSON.stringify(PAGES.map((page) => ({ page, html: sharePageHtml(page) }))));
  console.log(id);
}

// The real worker in a Node thread, its fetch answering `files` and nothing else.
function worker(after, script, files) {
  const thread = new Worker(`
    const { parentPort, workerData } = require('node:worker_threads');
    globalThis.self = globalThis;
    self.postMessage = (message, transfer) => parentPort.postMessage(message, transfer);
    globalThis.fetch = async (url) => workerData[url] ? new Response(workerData[url]) : new Response(null, { status: 404 });
    parentPort.on('message', (data) => self.onmessage({ data }));
    import(${JSON.stringify(script)});
  `, { eval: true, workerData: files });
  after.push(() => thread.terminate());
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

function fitView(bridge) {
  const source = fs.readFileSync(bridge, 'utf8');
  const from = source.indexOf('  var ROTATION');
  const to = source.indexOf('  // Past this angle');
  return new Function(`${source.slice(from, to)}; return { fit, placement, ROTATION, ANGLE };`)();
}

const BOXES = {
  tower: { min: [10, 70, -5], max: [12, 81, -3] },
  pad: { min: [-60, 64, -40], max: [59, 69, 39] },
  sphere: { min: [-150, 10, -150], max: [150, 310, 150] },
};

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

const CASES = {
  async 'the worker reads an address ending in .gz as gzip, as it streams, and the same world comes of it'(folder, after) {
    const { blocksBlueprint } = require(path.join(ROOT, 'server', 'blueprint'));
    const { renderMask } = require(path.join(ROOT, 'server', 'bluemap'));
    const { mesherPack } = require(path.join(ROOT, 'test', 'helpers', 'mesher-pack'));
    const blocks = [];
    for (let x = 0; x < 20; x += 1) for (let z = 0; z < 20; z += 1) blocks.push([x, 64 + ((x * z) % 5), z, x % 3 ? 'minecraft:stone' : 'minecraft:glass']);
    const blueprint = blocksBlueprint(blocks);
    // The viewer's own gzip member of the mesher, as mcbench_tools wrote it.
    const wasmGz = fs.readFileSync(path.join(folder, 'mesher.wasm.gz'));
    const wasm = zlib.gunzipSync(wasmGz);
    assert.deepEqual(wasm, fs.readFileSync(path.join(ROOT, 'renderer', 'mcbench-renderer.wasm')));
    const pack = mesherPack();
    const mask = renderMask({ min: [0, 64, 0], max: [19, 68, 19] });
    const script = path.join(folder, 'mesher-worker.mjs');
    const plain = worker(after, script, { 'm.wasm': wasm, 'p.bin': pack, 'b.bin': blueprint });
    const gzipped = worker(after, script, { 'm.wasm.gz': wasmGz, 'p.bin.gz': zlib.gzipSync(pack), 'b.bin.gz': zlib.gzipSync(blueprint) });
    const one = await plain({ type: 'load', wasm: 'm.wasm', pack: 'p.bin', blueprint: 'b.bin', mask, compare: false });
    const two = await gzipped({ type: 'load', wasm: 'm.wasm.gz', pack: 'p.bin.gz', blueprint: 'b.bin.gz', mask, compare: false });
    assert.ok(one.tiles.length > 0);
    assert.deepEqual(two.tiles, one.tiles);
    const [x, z] = one.tiles[0];
    const a = await plain({ type: 'snapshot', x, z });
    const b = await gzipped({ type: 'snapshot', x, z });
    assert.deepEqual(Buffer.from(b.bytes), Buffer.from(a.bytes), 'the same world, tile for tile');
    await assert.rejects(gzipped({ type: 'load', wasm: 'm.wasm.gz', pack: 'p.bin.gz', blueprint: 'gone.bin.gz', mask, compare: false }), /gone\.bin\.gz is unavailable \(HTTP 404\)/);
  },

  'the opening view sees the build\'s box from the south-east, 30 degrees down, whole and centered, at its middle height'(folder) {
    const { fit, ROTATION, ANGLE } = fitView(path.join(folder, 'share', 'bridge.js'));
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
        const fill = Math.max(...[0, 1].map((axis) => Math.max(...seen.map((point) => point[axis])) - Math.min(...seen.map((point) => point[axis])))) / 2;
        assert.ok(fill > (name === 'pad' && aspect > 1 ? 0.5 : 0.8), `${name} at ${aspect.toFixed(2)}: fills ${fill.toFixed(2)} of the frame`);
      }
    }
  },

  'a build shown after another is fitted from the direction the visitor looks: whole and centered at its middle height'(folder) {
    const { fit } = fitView(path.join(folder, 'share', 'bridge.js'));
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
  },

  'the share bridge loads only a build in its own site\'s b/ folder, and throws for any other address'(folder) {
    const source = fs.readFileSync(path.join(folder, 'share', 'bridge.js'), 'utf8');
    const from = source.indexOf('  function buildAddress');
    const to = source.indexOf('  var buildUrl');
    const read = new Function(`${source.slice(from, to)}; return buildAddress;`)();
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
  },

  'fly places the camera at the orbit view\'s eye, at distance 0, looking the same way: the same picture'(folder) {
    const { fit, placement, ROTATION, ANGLE } = fitView(path.join(folder, 'share', 'bridge.js'));
    for (const [rotation, angle] of [[ROTATION, ANGLE], [2.1, 0.2], [-3.3, 1.2]]) {
      for (const bounds of Object.values(BOXES)) {
        const view = fit(bounds, { fov: 75, aspect: 1 }, rotation, angle);
        const orbit = placement(view, 'orbit', rotation, angle);
        const fly = placement(view, 'fly', rotation, angle);
        assert.deepEqual(orbit, { position: view.target, distance: view.distance, rotation, angle });
        assert.deepEqual(fly, { position: view.eye, distance: 0, rotation, angle });
        const forward = [Math.sin(rotation) * Math.sin(angle), -Math.cos(angle), -Math.cos(rotation) * Math.sin(angle)];
        const eye = orbit.position.map((value, i) => value - forward[i] * orbit.distance);
        eye.forEach((value, i) => assert.ok(Math.abs(value - fly.position[i]) < 1e-6, 'one eye'));
      }
    }
    const level = fit(BOXES.tower, { fov: 75, aspect: 1 }, 0.4, Math.PI / 2);
    assert.ok(level.eye.every(Number.isFinite));
    const { seen } = cornersSeen({ target: level.eye.map((value, i) => value + [Math.sin(0.4), 0, -Math.cos(0.4)][i]), distance: 1 }, BOXES.tower, 0.4, Math.PI / 2, 1);
    for (const point of seen) assert.ok(Math.abs(point[0]) <= 1 && Math.abs(point[1]) <= 1, 'every corner in the frame');
  },

  'a share opens in free flight, and in orbit on a device whose only pointer is a finger'(folder) {
    const source = fs.readFileSync(path.join(folder, 'share', 'bridge.js'), 'utf8');
    const from = source.indexOf('  function openingMode');
    const to = source.indexOf('  // How far drawing');
    const openingMode = new Function(`${source.slice(from, to)}; return openingMode;`)();
    const asked = [];
    const device = (touchOnly) => (query) => { asked.push(query); return { matches: touchOnly }; };
    assert.equal(openingMode(device(false)), 'fly');
    assert.equal(openingMode(device(true)), 'orbit');
    assert.deepEqual([...new Set(asked)], ['(pointer: coarse) and (hover: none)']);
  },
};

async function main() {
  const [mode, a, b] = process.argv.slice(2);
  if (mode === 'diff') return diff(a, b);
  if (mode !== 'case' || !CASES[a]) throw new Error(`unknown: ${mode} ${a}`);
  const after = [];
  try {
    await CASES[a](b, after);
  } finally {
    for (const end of after) await end();
  }
  console.log('ok');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
