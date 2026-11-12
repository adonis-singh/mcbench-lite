'use strict';

const { World } = require('./world');
const { token } = require('./auth');

// see_build screenshots: a headless Chromium runs web/shot.html, a deepslate
// pipeline with custom shaders, ambient occlusion and voxel lighting. A shot
// is a named view or a camera of the model's own (eye and target), framing
// the build, a region, or a clip box (only the blocks inside it are drawn),
// in perspective or orthographic projection; straight orthographic views can
// carry a block grid labelled with coordinates.

// yaw: compass position of the eye (0 = south, 90 = east), pitch: degrees
// below the horizon.
const VIEWS = {
  iso_se: { yaw: 45, pitch: 30 },
  iso_sw: { yaw: -45, pitch: 30 },
  iso_ne: { yaw: 135, pitch: 30 },
  iso_nw: { yaw: -135, pitch: 30 },
  front: { yaw: 0, pitch: 0 },    // from the south
  back: { yaw: 180, pitch: 0 },   // from the north
  left: { yaw: 90, pitch: 0 },    // from the east
  right: { yaw: -90, pitch: 0 },  // from the west
  top: { yaw: 0, pitch: 90 },     // straight down, north up
};

// Long edge 2184 at 4:3 is 78 x 59 patches of 28 px, within the largest
// image Claude reads unscaled (4784 patches); other vision models scale it to
// their own input size.
const SIZES = { large: [2184, 1640], small: [1092, 820] };
// Shots are drawn at twice the size and scaled down, so distant blocks and
// edges are averaged instead of aliased.
const SUPERSAMPLE = 2;
const DEFAULT_FOV = 70;
const MARGIN = 1.06;
// As the screenshot page's own JPEGs (web/shot.mjs).
const MONTAGE_JPEG_QUALITY = 0.9;

const STAGE_TIMEOUT_MS = Number(process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS || 120_000);
if (!Number.isFinite(STAGE_TIMEOUT_MS) || STAGE_TIMEOUT_MS <= 0) throw new Error('Invalid screenshot stage timeout');
// The renderer: `renderer.name` is one of
//   none     no browser
//   opening  Chromium is being launched and the page loaded
//   ready    the page is loaded
//   closed   closeRenderer ended it: no browser opens again in this process
// opening and ready are one run: the same object, holding its `browser`
// (once launched) and `page` (a promise). A run ends once, in leave(): a
// crash, a page or browser that closed, a job that failed, stalled or was
// cancelled, and closeRenderer all end it there, and its browser is closed,
// so no Chromium outlives the run that launched it. A run that ended is
// `left`, and the renderer no longer.
let renderer = { name: 'none' };
let queue = Promise.resolve();

// Headless Chromium on Linux draws WebGL on the CPU (SwiftShader) unless
// ANGLE is pointed at the GPU; of its GPU backends, Vulkan gives the same
// pixels from one call to the next (EGL does not). macOS draws with Metal.
const GPU_ARGS = process.platform === 'linux' ? ['--use-angle=vulkan', '--enable-features=Vulkan', '--ignore-gpu-blocklist'] : [];

async function openPage(run) {
  const { chromium } = require('playwright-core');
  run.browser = await chromium.launch({
    executablePath: process.env.SHOT_CHROMIUM,
    args: [...GPU_ARGS, '--enable-unsafe-swiftshader'],
  });
  // A run left while Chromium launched has no use for it.
  if (run.name === 'left') {
    run.browser.close().catch(() => {});
    throw new Error('The screenshot renderer was discarded while it opened');
  }
  // The page is the app's own: it holds the app's access token (auth.js).
  const page = await run.browser.newPage({ viewport: { width: 1024, height: 768 }, extraHTTPHeaders: { authorization: `Bearer ${token()}` } });
  page.on('crash', () => leave(run));
  page.on('close', () => leave(run));
  run.browser.on('disconnected', () => leave(run));
  const port = Number(process.env.PORT || 3020);
  await page.goto(`http://127.0.0.1:${port}/shot.html`, { waitUntil: 'load' });
  if (run.name === 'opening') run.name = 'ready';
  return page;
}

// The run that renders now: the one open, or a new one.
function currentRun() {
  if (renderer.name === 'closed') throw new Error('The screenshot renderer was shut down');
  if (renderer.name === 'none') {
    const run = { name: 'opening', browser: null, page: null };
    run.page = openPage(run);
    run.page.catch(() => leave(run));
    renderer = run;
  }
  return renderer;
}

// Ends a run, once, and answers its browser's close. Nothing but
// closeRenderer waits for that, and it no longer than a stage: a crashed
// browser's close can hang, and must not block the next queued screenshot.
function leave(run) {
  if (run.name === 'left') return run.closing;
  run.name = 'left';
  if (renderer === run) renderer = { name: 'none' };
  run.closing = Promise.resolve(run.browser?.close());
  run.closing.catch(() => {});
  return run.closing;
}

// evaluate() has no Playwright timeout, and renderer crashes can leave it pending.
// Race each stage against explicit lifecycle events and an independent Node timer.
async function stage(label, operation, page, signal, timings) {
  const started = process.hrtime.bigint();
  try {
    return await runStage(label, operation, page, signal);
  } finally {
    if (timings) timings[label] = (timings[label] || 0) + Number(process.hrtime.bigint() - started) / 1e6;
  }
}

async function runStage(label, operation, page, signal) {
  let rejectFailure;
  const failure = new Promise((_, reject) => { rejectFailure = reject; });
  const crash = () => rejectFailure(new Error(`Screenshot ${label}: renderer crashed`));
  const closed = () => rejectFailure(new Error(`Screenshot ${label}: renderer disconnected`));
  const pageError = error => rejectFailure(new Error(`Screenshot ${label}: ${error.message || error}`));
  const abort = () => rejectFailure(signal.reason || new Error('Screenshot cancelled'));
  const browser = page?.context().browser();
  page?.on('crash', crash);
  page?.on('close', closed);
  page?.on('pageerror', pageError);
  browser?.on('disconnected', closed);
  signal?.addEventListener('abort', abort, { once: true });
  const timer = setTimeout(() => rejectFailure(new Error(`Screenshot ${label} stalled for ${STAGE_TIMEOUT_MS / 1000}s; renderer reset, screenshot unavailable`)), STAGE_TIMEOUT_MS);
  try {
    signal?.throwIfAborted();
    return await Promise.race([Promise.resolve().then(operation), failure]);
  } finally {
    clearTimeout(timer);
    page?.off?.('crash', crash);
    page?.off?.('close', closed);
    page?.off?.('pageerror', pageError);
    browser?.off?.('disconnected', closed);
    signal?.removeEventListener('abort', abort);
  }
}

const round = (value) => Math.round(value * 100) / 100;
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

function point(value, name) {
  if (!Array.isArray(value) || value.length !== 3 || !value.every(Number.isFinite)) throw new Error(`${name} must be [x, y, z]`);
  return value;
}

// A {from, to} box of blocks, inclusive.
function blockBox(value, name) {
  if (!value || typeof value !== 'object') throw new Error(`${name} must be {from: [x, y, z], to: [x, y, z]}`);
  const [a, b] = [point(value.from, `${name}.from`), point(value.to, `${name}.to`)];
  if (![...a, ...b].every(Number.isInteger)) throw new Error(`${name} corners must be whole block coordinates`);
  return { min: a.map((v, i) => Math.min(v, b[i])), max: a.map((v, i) => Math.max(v, b[i])) };
}

function intersect(a, b, name) {
  const box = { min: a.min.map((v, i) => Math.max(v, b.min[i])), max: a.max.map((v, i) => Math.min(v, b.max[i])) };
  if (box.min.some((v, i) => v > box.max[i])) throw new Error(`Nothing to draw: ${name} holds no blocks of the build`);
  return box;
}

// The world axis a camera axis lies along, or null.
function axisOf(vector) {
  const index = vector.findIndex((v) => Math.abs(Math.abs(v) - 1) < 1e-9);
  return index < 0 ? null : { index, sign: Math.sign(vector[index]), name: 'xyz'[index] };
}

// Pixel lines of block edges and labels of block coordinates, on a straight
// orthographic shot: along the image's width the world axis `across`, down
// its height the world axis `down`.
function gridFor({ across, down, center, halfWidth, halfHeight, width, height }) {
  const perBlock = width / (2 * halfWidth);
  const step = [1, 2, 5, 10, 20, 25, 50, 100, 200, 500, 1000].find((value) => value * perBlock >= 36) || 1000;
  const lines = (axis, half, size, flip) => {
    const at = (b) => ((b - center[axis.index]) * axis.sign * flip / (2 * half) + 0.5) * size;
    const low = Math.floor(center[axis.index] - half);
    const high = Math.ceil(center[axis.index] + half);
    const edges = [];
    const labels = [];
    for (let b = low; b <= high; b += 1) {
      if (perBlock >= 6 || b % step === 0) edges.push({ at: at(b), major: b % step === 0 });
      if (b % step === 0) labels.push({ at: at(b + 0.5), text: String(b) });
    }
    return { edges, labels };
  };
  const columns = lines(across, halfWidth, width, 1);
  const rows = lines(down, halfHeight, height, 1);
  const name = (axis) => `${axis.sign > 0 ? '+' : '-'}${axis.name}`;
  return { columns, rows, legend: `${name(across)} →   ${name(down)} ↓   grid: 1 block, labels every ${step}`, step };
}

// Points a shot frames: the corners of each chunk's occupied box within
// `box`, so a sparse build is framed by its blocks, not its bounding box.
function framingPoints(world, box) {
  const points = [];
  for (const [key, chunk] of world.chunks) {
    const origin = World.coordsOf(key).map((c) => c * 16);
    const local = chunk.bounds();
    const min = [0, 1, 2].map((i) => Math.max(origin[i] + local[i], box.min[i]));
    const max = [0, 1, 2].map((i) => Math.min(origin[i] + local[i + 3], box.max[i]) + 1);
    if (min.some((v, i) => v >= max[i])) continue;
    for (const x of [min[0], max[0]]) for (const y of [min[1], max[1]]) for (const z of [min[2], max[2]]) points.push([x, y, z]);
  }
  return points;
}

// The camera of a shot of `world`: viewer convention (forward = [cos p sin
// y, -sin p, -cos p cos y], eye = focus - forward * focusDist), plus a lens.
// `sizes` are see_build's unless a caller has its own (a shared build's
// square preview, share.js).
function resolveShot(spec, world, sizes = SIZES) {
  const bounds = world.bounds();
  if (!bounds) throw new Error('Nothing to render (empty world)');
  const known = new Set(['view', 'eye', 'target', 'region', 'clip', 'projection', 'fov', 'grid', 'size']);
  for (const key of Object.keys(spec)) if (!known.has(key)) throw new Error(`Unknown shot field "${key}"`);
  const size = spec.size ?? 'large';
  if (!sizes[size]) throw new Error(`size must be ${Object.keys(sizes).join(' or ')}`);
  const [width, height] = sizes[size];
  const projection = spec.projection ?? 'perspective';
  if (projection !== 'perspective' && projection !== 'orthographic') throw new Error('projection must be perspective or orthographic');
  const ortho = projection === 'orthographic';
  if (ortho && spec.fov !== undefined) throw new Error('fov is for perspective shots');
  const fov = spec.fov ?? DEFAULT_FOV;
  if (!Number.isFinite(fov) || fov < 5 || fov > 120) throw new Error('fov must be between 5 and 120 degrees');
  const clip = spec.clip ? intersect(blockBox(spec.clip, 'clip'), bounds, 'clip') : null;
  const shown = clip || bounds;
  const region = spec.region ? intersect(blockBox(spec.region, 'region'), shown, 'region') : shown;

  let yaw;
  let pitch;
  let target = null;
  let distance = 0;
  if (spec.eye !== undefined || spec.target !== undefined) {
    if (spec.view !== undefined) throw new Error('A shot takes a view or an eye and target, not both');
    const eye = point(spec.eye, 'eye');
    target = point(spec.target, 'target');
    const d = target.map((v, i) => v - eye[i]);
    distance = Math.hypot(...d);
    if (distance === 0) throw new Error('eye and target must differ');
    if (!ortho && spec.region) throw new Error('region frames a view; a perspective shot from your own eye frames itself');
    pitch = Math.asin(-d[1] / distance);
    yaw = Math.atan2(d[0], -d[2]);
  } else {
    const view = VIEWS[spec.view ?? 'iso_se'];
    if (!view) throw new Error(`Unknown view "${spec.view}" (valid: ${Object.keys(VIEWS).join(', ')})`);
    yaw = -(view.yaw * Math.PI) / 180;
    pitch = (view.pitch * Math.PI) / 180;
  }
  const right = [Math.cos(yaw), 0, Math.sin(yaw)];
  const up = [Math.sin(yaw) * Math.sin(pitch), Math.cos(pitch), -Math.cos(yaw) * Math.sin(pitch)];
  const forward = [Math.cos(pitch) * Math.sin(yaw), -Math.sin(pitch), -Math.cos(pitch) * Math.cos(yaw)];
  for (const vector of [right, up, forward]) for (let i = 0; i < 3; i += 1) if (Math.abs(vector[i]) < 1e-12) vector[i] = 0;
  const points = framingPoints(world, region);
  if (points.length === 0) throw new Error('Nothing to draw: the framed box holds no blocks');
  // Plain loops: a large world has more points than a call takes as spread
  // arguments.
  const lowest = [Infinity, Infinity, Infinity];
  const highest = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let i = 0; i < 3; i += 1) {
      if (p[i] < lowest[i]) lowest[i] = p[i];
      if (p[i] > highest[i]) highest[i] = p[i];
    }
  }
  const boxCenter = lowest.map((v, i) => (v + highest[i]) / 2);
  // Each point relative to the box center, along right, up and forward.
  const offsets = points.map((p) => {
    const d = p.map((v, i) => v - boxCenter[i]);
    return [dot(d, right), dot(d, up), dot(d, forward)];
  });
  const range = (axis) => {
    let low = Infinity;
    let high = -Infinity;
    for (const o of offsets) {
      if (o[axis] < low) low = o[axis];
      if (o[axis] > high) high = o[axis];
    }
    return [low, high];
  };
  const aspect = width / height;

  let camera;
  let described;
  let grid = null;
  if (ortho) {
    const [[r0, r1], [u0, u1], [f0, f1]] = [range(0), range(1), range(2)];
    const center = boxCenter.map((v, i) => v + right[i] * (r0 + r1) / 2 + up[i] * (u0 + u1) / 2);
    const halfHeight = Math.max((u1 - u0) / 2, (r1 - r0) / 2 / aspect) * MARGIN;
    const halfWidth = halfHeight * aspect;
    const depth = -f0 + 2;
    camera = { yaw, pitch, focus: center, focusDist: depth, ortho: { halfWidth, halfHeight, near: 0.05, far: depth + f1 + 2 } };
    described = { projection, looking: forward.map(round), center: center.map(round), widthBlocks: round(2 * halfWidth), heightBlocks: round(2 * halfHeight) };
    if (spec.grid) {
      const across = axisOf(right);
      const down = axisOf(up.map((v) => -v));
      if (!across || !down) throw new Error('grid needs a straight orthographic shot: view top, front, back, left or right, or an eye and target along one axis');
      grid = gridFor({ across, down, center, halfWidth, halfHeight, width, height });
      described.grid = grid.legend;
    }
  } else {
    if (spec.grid) throw new Error('grid is for orthographic shots');
    const tanHalfFov = Math.tan((fov / 2) * Math.PI / 180);
    if (!target) {
      // The nearest eye on the line through the box center that sees every
      // point within the field of view.
      target = boxCenter;
      const tan = tanHalfFov / MARGIN;
      // A point's depth from the eye is distance + f.
      distance = -Infinity;
      for (const [r, u, f] of offsets) distance = Math.max(distance, Math.max(Math.abs(u) / tan, Math.abs(r) / (tan * aspect), 1) - f);
    }
    camera = { yaw, pitch, focus: target, focusDist: distance, fov };
    described = { projection, eye: target.map((v, i) => round(v - forward[i] * distance)), target: target.map(round), fov };
  }
  if (clip) described.clip = { from: clip.min, to: clip.max };
  return { width, height, camera, clip, grid, described };
}

const groupKey = (shot) => `${shot.tick ?? ''}@${shot.clip ? `${shot.clip.min.join(',')}:${shot.clip.max.join(',')}` : ''}`;

// Renders resolved shots (resolveShot, each with the `tick` of the engine
// world it shows) of a conversation's engine world. The shots that share a
// tick and a clip share one world load; calls are serialized through
// one warm page and one canvas. Each shot is a JPEG.
function renderShots(conversationId, shots, signal, timings = {}) {
  signal?.throwIfAborted();
  const groups = new Map();
  shots.forEach((shot, index) => {
    const key = groupKey(shot);
    if (!groups.has(key)) groups.set(key, { clip: shot.clip, tick: shot.tick, shots: [] });
    groups.get(key).shots.push(index);
  });
  let activePage, run;
  const discardPage = () => { if (run) leave(run); };
  const job = queue.then(async () => {
    // A lost WebGL context invalidates all cached GPU resources. Start a fresh
    // page and reload the complete world; never reuse its chunk-version cache.
    for (let attempt = 0; ; attempt++) {
      try {
        return await capture();
      } catch (error) {
        if (signal?.aborted || attempt > 0 || !String(error.message).includes('Screenshot WebGL context lost')) throw error;
        discardPage();
        activePage = null;
      }
    }
  });
  async function capture() {
    signal?.throwIfAborted();
    run = currentRun();
    const page = await stage('startup', () => run.page, null, signal, timings);
    activePage = page;
    signal?.throwIfAborted();
    await stage('initialization', () => page.evaluate(() => window.__shot.init()), page, signal, timings);
    const buffers = new Array(shots.length);
    for (const group of groups.values()) {
      const clip = group.clip && { from: group.clip.min, to: group.clip.max };
      await stage('world loading', () => page.evaluate(([id, box, tick]) => window.__shot.load(id, box, tick), [conversationId, clip, group.tick]), page, signal, timings);
      await stage('meshing', () => page.evaluate(() => window.__shot.settle()), page, signal, timings);
      const frames = group.shots.map((index) => {
        const { camera, width, height, grid } = shots[index];
        return { camera, width, height, supersample: SUPERSAMPLE, grid };
      });
      // All shots of the group in one call (one task in the page).
      const dataUrls = await stage('capture', () => page.evaluate((specs) => window.__shot.frames(specs), frames), page, signal, timings);
      group.shots.forEach((index, i) => {
        const dataUrl = dataUrls[i];
        if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:image/jpeg;base64,')) {
          throw new Error('Screenshot renderer returned no JPEG image');
        }
        buffers[index] = Buffer.from(dataUrl.slice('data:image/jpeg;base64,'.length), 'base64');
      });
    }
    return buffers;
  }
  // Keep the chain alive after failures, but reset the page so the next call
  // starts clean instead of inheriting a wedged renderer.
  queue = job.catch(discardPage);
  if (!signal) return job;
  return new Promise((resolve, reject) => {
    const abort = () => {
      if (activePage) leave(run);
      reject(signal.reason);
    };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
    job.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
  });
}

// The cells of a square card of `side` pixels holding `count` shots, in
// order: one row for 1 or 2, else two, the top one holding ceil(count / 2);
// each row's cells split the width evenly. Cells are { x, y, width, height }.
function montageCells(count, side) {
  if (!Number.isInteger(count) || count < 1) throw new Error('A card holds at least one shot');
  const rows = count <= 2 ? [count] : [Math.ceil(count / 2), Math.floor(count / 2)];
  const edge = (i, parts) => Math.round((i * side) / parts);
  return rows.flatMap((columns, row) => Array.from({ length: columns }, (_, column) => ({
    x: edge(column, columns),
    y: edge(row, rows.length),
    width: edge(column + 1, columns) - edge(column, columns),
    height: edge(row + 1, rows.length) - edge(row, rows.length),
  })));
}

// One JPEG of `width` x `height` with each JPEG of `tiles` drawn at its cell
// (montageCells), drawn on a canvas in the screenshot page.
function composeMontage(tiles, cells, [width, height]) {
  if (tiles.length !== cells.length) throw new Error('A card needs one shot per cell');
  let run;
  const job = queue.then(async () => {
    run = currentRun();
    const page = await stage('startup', () => run.page, null);
    const dataUrl = await stage('montage', () => page.evaluate(async ({ images, boxes, size, quality }) => {
      const canvas = document.createElement('canvas');
      [canvas.width, canvas.height] = size;
      const context = canvas.getContext('2d');
      for (let i = 0; i < images.length; i += 1) {
        const image = new Image();
        image.src = images[i];
        await image.decode();
        const { x, y, width: w, height: h } = boxes[i];
        context.drawImage(image, x, y, w, h);
      }
      return canvas.toDataURL('image/jpeg', quality);
    }, {
      images: tiles.map((tile) => `data:image/jpeg;base64,${tile.toString('base64')}`),
      boxes: cells,
      size: [width, height],
      quality: MONTAGE_JPEG_QUALITY,
    }), page);
    if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:image/jpeg;base64,')) throw new Error('Screenshot renderer returned no JPEG image');
    return Buffer.from(dataUrl.slice('data:image/jpeg;base64,'.length), 'base64');
  });
  queue = job.catch(() => { if (run) leave(run); });
  return job;
}

// Starts the renderer and its viewer before the first screenshot needs them
// (Chromium's launch and the viewer's shaders take seconds). Its stages are
// bounded as a screenshot's are, and one that fails or stalls ends the run,
// so the queue behind it moves on.
function warmRenderer() {
  let run;
  const job = queue.then(async () => {
    run = currentRun();
    const page = await stage('startup', () => run.page, null);
    await stage('initialization', () => page.evaluate(() => window.__shot.init()), page);
  });
  queue = job.catch(() => { if (run) leave(run); });
  return job;
}

// Closes the browser for good: a job queued meanwhile finds no page. The job
// under way finishes on the run that was open, which then ends. The close is
// bounded as a stage is, so a Chromium whose close hangs fails this instead
// of hanging the server's shutdown: that browser is left to Playwright,
// which kills the browsers it launched when this process exits.
async function closeRenderer() {
  const run = renderer;
  renderer = { name: 'closed' };
  await queue;
  if (!['opening', 'ready'].includes(run.name)) return;
  let timer;
  const stalled = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`The screenshot browser did not close within ${STAGE_TIMEOUT_MS / 1000}s`)), STAGE_TIMEOUT_MS);
  });
  try {
    await Promise.race([leave(run), stalled]);
  } finally {
    clearTimeout(timer);
  }
}

module.exports = { resolveShot, renderShots, montageCells, composeMontage, VIEWS, SIZES, warmRenderer, closeRenderer };
