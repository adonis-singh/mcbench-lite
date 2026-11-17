'use strict';

const path = require('path');
const { spawn } = require('child_process');
const { World } = require('./world');
const draw = require('./draw');
const setup = require('./setup');
const shotWorlds = require('./shot-world');
const { minecraftLive } = require('./minecraft-live');

// see_build screenshots: mcbench-draw (native/draw), a native program kept
// running beside the server, draws them on the GPU from the engine's world
// with the mesher's quads (renderer/), see_build's shading, sky and light.
// A shot is a named view or a camera of the model's own (eye and target),
// framing the build, a region, or a clip box (only the blocks inside it are
// drawn), in perspective or orthographic projection; straight orthographic
// views can carry a block grid labelled with coordinates. The camera is
// resolved here (resolveShot) and drawn there as resolved.

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
// Every picture is a JPEG of this quality: a large shot as PNG runs to
// megabytes, and every request carries the latest screenshot batches.
const JPEG_QUALITY = 90;

const STAGE_TIMEOUT_MS = Number(process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS || 120_000);
if (!Number.isFinite(STAGE_TIMEOUT_MS) || STAGE_TIMEOUT_MS <= 0) throw new Error('Invalid screenshot stage timeout');
// A helper with nothing to do for this long is stopped, so an idle app holds
// no renderer memory (0.8 GB on the M3, 2.2 GB on an RTX 5090 after a large
// build); the next call starts it again (0.2 to 0.6 s) and sends its world
// again (0.6 to 0.8 s for 2 million blocks). Two minutes keeps it through a
// turn, whose tool calls come seconds to tens of seconds apart, and lets it go
// soon after the turn ends.
const IDLE_MS = Number(process.env.MCBENCH_SHOT_IDLE_MS || 120_000);
if (!Number.isFinite(IDLE_MS) || IDLE_MS <= 0) throw new Error('Invalid screenshot renderer idle time');

// The renderer: `renderer.name` is one of
//   none      no helper
//   starting  the helper is being installed, started and given its assets
//             (hello)
//   ready     the helper answered hello
//   closed    closeRenderer ended it: no helper starts again in this process
// starting and ready are one run: the same object, holding its `child` (once
// spawned), `ready` (a promise of its hello) and the requests it has not
// answered (`replies`, by id). A run ends once, in leave(): a helper that
// exited (a crash, a lost device), a request that stalled, and closeRenderer
// all end it there, and its process is killed, so no helper outlives the run
// that started it. A run that ended is `left`, and the renderer no longer.
let renderer = { name: 'none' };
let queue = Promise.resolve();
// Calls queued or under way, and the timer that stops an idle helper.
let busy = 0;
let idle = null;

// Queues `work` behind every call before it. Once the queue is empty, the
// helper is stopped after IDLE_MS unless another call comes first.
function enqueue(work) {
  busy += 1;
  clearTimeout(idle);
  const job = queue.then(work);
  queue = job.catch(() => {});
  queue.then(() => {
    busy -= 1;
    if (busy > 0) return;
    const run = renderer;
    idle = setTimeout(() => { if (busy === 0 && renderer === run && run.name === 'ready') leave(run); }, IDLE_MS);
    idle.unref();
  });
  return job;
}
// The adapter the helper said it draws on, logged when it is first told and
// when a restarted helper tells another.
let adapterLine = null;

// A failure of the helper itself, after which it is started again and the
// call tried once more: it exited, or it lost its graphics device (it exits
// then too, PROTOCOL.md).
const restartable = (error) => Object.assign(error, { restart: true });
// A failure the helper reported or caused, which a stage names
// ("Screenshot <stage>: ..."), as today's page's crashes and page errors were
// named. Other failures (an install that failed) reach the caller as they are.
const named = (error) => Object.assign(error, { named: true });
// The words a model has been given since before the helper for the same
// situations (server/shot.js and web/shot.mjs on main): a lost graphics
// device, and a world that is not placed for the conversation.
const LOST_DEVICE = 'page.evaluate: Error: Screenshot WebGL context lost';

function startRun() {
  const run = { name: 'starting', child: null, replies: new Map(), ids: 0, exited: null, sent: new Map() };
  // Started: installed, given its files' paths and spawned. Ready: it
  // answered hello.
  run.started = (async () => {
    const program = await draw.installDraw();
    const assets = await draw.helperAssets(program);
    // A run left while the helper was installed has no use for it.
    if (run.name === 'left') throw new Error('The screenshot renderer was discarded while it opened');
    // In its own folder: SwiftShader, the software floor, reads a
    // SwiftShader.ini from the folder it runs in.
    const child = spawn(program, ['serve'], { cwd: path.dirname(program), stdio: ['pipe', 'pipe', 'pipe'] });
    run.child = child;
    run.exited = new Promise((resolve) => child.once('close', (code, signal) => resolve({ code, signal })));
    run.exited.then(({ code, signal }) => {
      if (run.name !== 'left') console.error(`[see_build] the renderer exited (${signal ? `signal ${signal}` : `exit status ${code}`})`);
      leave(run, named(restartable(new Error('renderer crashed'))));
    });
    child.once('error', (error) => leave(run, named(new Error(`renderer could not be started: ${error.message}`))));
    // A helper that exited fails its requests through leave(); a write to it
    // meanwhile fails here first.
    child.stdin.on('error', () => {});
    const reader = new draw.FrameReader((header, payloads) => answer(run, header, payloads));
    child.stdout.on('data', (chunk) => {
      try {
        reader.push(chunk);
      } catch (error) {
        leave(run, named(new Error(`renderer sent a frame that cannot be read: ${error.message}`)));
      }
    });
    let line = '';
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (text) => {
      const lines = (line + text).split('\n');
      line = lines.pop();
      for (const entry of lines) if (entry) console.log(`[see_build] ${entry}`);
    });
    return assets;
  })();
  run.ready = run.started.then(async (assets) => {
    // What it keeps between calls is bounded by its own default
    // (PROTOCOL.md memoryLimit), which the server does not change.
    const { header } = await request(run, { type: 'hello', assets });
    const { name, backend, deviceType, driver, driverInfo, software } = header.adapter;
    const told = `[see_build] ${header.version} draws on ${name} (${backend}, ${deviceType}${driver ? `, ${[driver, driverInfo].filter(Boolean).join(' ')}` : ''})${software ? ', software: true: a CPU rasteriser draws the screenshots' : ''}`;
    if (told !== adapterLine) console.log(told);
    adapterLine = told;
    if (run.name === 'starting') run.name = 'ready';
  });
  run.ready.catch(() => leave(run));
  return run;
}

// The run that renders now: the one open, or a new one.
function currentRun() {
  if (renderer.name === 'closed') throw new Error('The screenshot renderer was shut down');
  if (renderer.name === 'none') renderer = startRun();
  return renderer;
}

// Ends a run, once: its helper is killed and every request it had not
// answered fails with `error` (or that the renderer was reset). Answers the
// helper's exit.
function leave(run, error = new Error('renderer reset')) {
  if (run.name === 'left') return run.exited || Promise.resolve();
  run.name = 'left';
  // A request made of it later fails as the run ended: a helper that
  // exited between two requests of a call is a crash, which the call
  // retries on a fresh helper as one that exited during a request.
  run.ended = error;
  if (renderer === run) renderer = { name: 'none' };
  for (const { reject } of run.replies.values()) reject(error);
  run.replies.clear();
  run.child?.kill('SIGKILL');
  return run.exited || Promise.resolve();
}

// Sends one request; resolves with its reply ({ header, payloads }) or
// rejects with its error, the reply's flags on it (deviceLost: the helper
// restarts; unknownWorld: the world must be sent).
function request(run, header, payloads = [], onId = () => {}) {
  if (run.name === 'left') return Promise.reject(run.ended);
  const id = ++run.ids;
  onId(id);
  return new Promise((resolve, reject) => {
    run.replies.set(id, { resolve, reject });
    run.child.stdin.write(draw.encodeFrame({ id, ...header }, payloads));
  });
}

function answer(run, header, payloads) {
  const waiting = run.replies.get(header.id);
  if (!waiting) return leave(run, named(new Error(`renderer answered request ${JSON.stringify(header.id)}, which it was not asked`)));
  run.replies.delete(header.id);
  if (header.ok === true) return waiting.resolve({ header, payloads });
  // A lost device is told as today's page told a lost WebGL context; the
  // helper's reason goes to the log.
  if (header.deviceLost) console.error(`[see_build] ${header.error}`);
  const error = header.deviceLost ? restartable(new Error(LOST_DEVICE)) : named(new Error(String(header.error)));
  if (header.unknownWorld) error.unknownWorld = true;
  waiting.reject(error);
}

// Each step of a call is bounded: a helper that stops answering is killed
// and the step fails, so the queue behind it moves on. An abort fails the
// step at once with the signal's reason; the helper's own job is cancelled
// by the caller (renderShots). A failure names the step.
async function stage(label, operation, run, signal, timings) {
  const started = process.hrtime.bigint();
  let timer;
  let abort;
  try {
    signal?.throwIfAborted();
    const stalled = new Promise((_, reject) => {
      timer = setTimeout(() => {
        reject(new Error(`Screenshot ${label} stalled for ${STAGE_TIMEOUT_MS / 1000}s; renderer reset, screenshot unavailable`));
        leave(run);
      }, STAGE_TIMEOUT_MS);
    });
    const aborted = new Promise((_, reject) => {
      abort = () => reject(signal.reason || new Error('Screenshot cancelled'));
      signal?.addEventListener('abort', abort, { once: true });
    });
    return await Promise.race([Promise.resolve().then(operation), stalled, aborted]).catch((error) => {
      if (error.named) error.message = `Screenshot ${label}: ${error.message}`;
      throw error;
    });
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
    if (timings) timings[label] = (timings[label] || 0) + Number(process.hrtime.bigint() - started) / 1e6;
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

// The corners of each entity's box (its width across x and z, its height up
// from its position), cut to `box` when given: an entity is framed with the
// blocks, also past them (a minecart on a rail at the build's edge), but
// not past a clip or region the shot was given.
function entityPoints(entities, box) {
  const points = [];
  for (const { pos: [x, y, z], width, height } of entities) {
    let min = [x - width / 2, y, z - width / 2];
    let max = [x + width / 2, y + height, z + width / 2];
    if (box) {
      min = min.map((v, i) => Math.max(v, box.min[i]));
      max = max.map((v, i) => Math.min(v, box.max[i] + 1));
      if (min.some((v, i) => v > max[i])) continue;
    }
    for (const px of [min[0], max[0]]) for (const py of [min[1], max[1]]) for (const pz of [min[2], max[2]]) points.push([px, py, pz]);
  }
  return points;
}

// The camera of a shot of `world` and its `entities` (the engine's list at
// the tick shot): viewer convention (forward = [cos p sin y, -sin p, -cos p
// cos y], eye = focus - forward * focusDist), plus a lens. `sizes` are
// see_build's unless a caller has its own (a shared build's square preview,
// share.js).
function resolveShot(spec, world, entities, sizes = SIZES) {
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
  for (const p of entityPoints(entities, spec.region ? region : clip)) points.push(p);
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
  // `world` is what renderShots draws: the settle it was resolved on.
  return { width, height, camera, clip, grid, described, world };
}

// Runs `attempt`, and once more after `discard` when the first run fails on
// the helper itself (it exited, or lost its graphics device): a fresh helper
// holds no world, so the second run sends its worlds again. A second such
// failure, any other error or an abort fails as it is.
async function retryOnce(attempt, discard, signal) {
  for (let tries = 0; ; tries++) {
    try {
      return await attempt();
    } catch (error) {
      if (signal?.aborted || tries > 0 || !error.restart) throw error;
      discard();
    }
  }
}

// What a shot is to the helper: resolveShot's fields it draws.
const drawn = ({ width, height, camera, clip, grid }) => ({ width, height, camera, clip, grid });
const add = (timings, name, value) => { timings[name] = (timings[name] || 0) + value; };
const isJpeg = (buffer) => buffer.length > 3 && buffer[0] === 0xff && buffer[1] === 0xd8 && buffer[2] === 0xff;

// Renders resolved shots (resolveShot, each with the `tick` of the engine
// world it shows) of a conversation's engine world. The shots of one settled
// world are one request; a world the helper does not keep is sent to it
// first, once (the engine's blueprint and entities, shot-world.js sceneOf).
// Calls are serialized through the one helper. Each shot is a JPEG.
//
// A world sent names as its `base` the conversation's world sent before it,
// of the same tick when there is one, so the helper re-meshes only what
// differs (PROTOCOL.md). The base is dropped (`keepBase` false) when it is an
// older edition of the build: after an edit nothing draws the old build
// again (agent.js see_build and share.js draw the conversation's current
// world). It is kept when it is the same edition at another tick: a
// see_build of several ticks draws each, and the next one may again. Worlds
// of other conversations (a bench's members, share.js) are never bases. A
// base the helper no longer holds is answered `unknownWorld`: the world is
// then sent whole.
function renderShots(conversationId, shots, signal, timings = {}) {
  signal?.throwIfAborted();
  let run;
  let active = null;
  const job = enqueue(() => retryOnce(capture, () => leave(run), signal));
  async function capture() {
    signal?.throwIfAborted();
    // One world per settle: its shots in one request, in their order.
    const groups = new Map();
    shots.forEach((shot, index) => {
      const scene = shotWorlds.sceneOf(shot.world);
      if (!groups.has(scene)) groups.set(scene, { name: `${conversationId}/tick ${shot.tick}/${scene.serial}`, tick: shot.tick, scene, shots: [] });
      groups.get(scene).shots.push(index);
    });
    run = currentRun();
    await stage('startup', () => run.started, run, signal, timings);
    await stage('initialization', () => run.ready, run, signal, timings);
    // The worlds this helper was sent of the conversation: by tick, and the last.
    if (!run.sent.has(conversationId)) run.sent.set(conversationId, { ticks: new Map(), last: null });
    const sent = run.sent.get(conversationId);
    const buffers = new Array(shots.length);
    for (const { name, tick, scene, shots: indices } of groups.values()) {
      const send = async () => {
        const base = sent.ticks.get(tick) || sent.last;
        const world = (edit) => stage('world loading', () => request(run, {
          type: 'world', world: name, yOffset: minecraftLive.yOffset, ...(edit ? { base: edit.name, keepBase: edit.edition === scene.edition } : {}),
        }, [scene.blueprint, Buffer.from(JSON.stringify(scene.entities))]), run, signal, timings);
        const edit = base && base.name !== name ? base : null;
        let reply;
        try {
          reply = await world(edit);
        } catch (error) {
          if (!error.unknownWorld) throw error;
          reply = await world(null);
        }
        // A base the helper dropped is no one's base again.
        if (edit && edit.edition !== scene.edition) for (const [at, held] of sent.ticks) if (held.name === edit.name) sent.ticks.delete(at);
        const { header } = reply;
        for (const phase of ['read', 'mesh', 'entities', 'upload']) add(timings, `world ${phase}`, header.timings[phase]);
        // A texture the atlas lacks leaves its quads undrawn: said, not hidden.
        if (header.missingTextures.length) console.error(`[see_build] ${name}: the block atlas has no ${header.missingTextures.join(', ')}; their faces are not drawn`);
        sent.ticks.set(tick, { name, edition: scene.edition });
        sent.last = { name, edition: scene.edition };
      };
      const drawShots = () => stage('capture', () => request(run, {
        type: 'shots', world: name, shots: indices.map((index) => drawn(shots[index])), supersample: SUPERSAMPLE, format: { type: 'jpeg', quality: JPEG_QUALITY },
      }, [], (id) => { active = { run, id }; }), run, signal, timings).finally(() => { active = null; });
      let reply;
      try {
        reply = await drawShots();
      } catch (error) {
        if (!error.unknownWorld) throw error;
        await send();
        reply = await drawShots();
      }
      if (reply.payloads.length !== indices.length || !reply.payloads.every(isJpeg)) throw new Error('Screenshot renderer returned no JPEG image');
      for (const shot of reply.header.shots) for (const phase of ['scene', 'draw', 'readback', 'grid', 'encode']) add(timings, `shot ${phase}`, shot.timings[phase]);
      indices.forEach((index, i) => { buffers[index] = reply.payloads[i]; });
    }
    return buffers;
  }
  if (!signal) return job;
  return new Promise((resolve, reject) => {
    // A call cancelled while queued never reaches the helper; one under way
    // has its job cancelled in the helper, which stays up.
    const abort = () => {
      if (active && active.run.name !== 'left') request(active.run, { type: 'cancel', job: active.id }).catch(() => {});
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
// (montageCells), drawn by the helper.
function composeMontage(tiles, cells, [width, height]) {
  if (tiles.length !== cells.length) throw new Error('A card needs one shot per cell');
  let run;
  return enqueue(() => retryOnce(async () => {
    run = currentRun();
    await stage('startup', () => run.started, run);
    await stage('initialization', () => run.ready, run);
    const { payloads } = await stage('montage', () => request(run, { type: 'montage', size: [width, height], cells, quality: JPEG_QUALITY }, tiles), run);
    if (payloads.length !== 1 || !isJpeg(payloads[0])) throw new Error('Screenshot renderer returned no JPEG image');
    return payloads[0];
  }, () => leave(run)));
}

// Starts the helper before the first screenshot needs it (its graphics
// device, assets and pipelines take a moment). Its start is bounded as a
// screenshot's stages are, and one that fails or stalls ends the run, so the
// queue behind it moves on. A helper that exits or loses its device while
// starting is started once more, as a screenshot's would be.
function warmRenderer() {
  let run;
  return enqueue(() => retryOnce(async () => {
    run = currentRun();
    await stage('startup', () => run.started, run);
    await stage('initialization', () => run.ready, run);
  }, () => leave(run)));
}

// The first start's step `draw` (setup.js): the helper installed, the
// viewer's bake it draws with made, and the helper started, so a helper that
// cannot draw on this machine (no graphics adapter it may use) fails the step
// with its own sentence, which see_build then answers too. Retried with the
// other steps (setup.retry), never on its own.
function startRenderer() {
  return setup.track('draw', async () => {
    await draw.installDraw();
    await minecraftLive.prepareViewer();
    await warmRenderer();
  });
}

// Ends the helper for good: a job queued meanwhile finds none. The job under
// way finishes on the run that was open, which then ends: its stdin is
// closed, and the helper exits once it answered. A helper that does not exit
// within a stage's time is killed, and this fails saying so.
async function closeRenderer() {
  const run = renderer;
  renderer = { name: 'closed' };
  clearTimeout(idle);
  await queue;
  if (!['starting', 'ready'].includes(run.name)) return;
  if (!run.child) return leave(run);
  run.child.stdin.end();
  let timer;
  const stalled = new Promise((resolve) => { timer = setTimeout(resolve, STAGE_TIMEOUT_MS, 'stalled'); });
  try {
    if (await Promise.race([run.exited, stalled]) === 'stalled') {
      await leave(run);
      throw new Error(`The screenshot renderer did not close within ${STAGE_TIMEOUT_MS / 1000}s`);
    }
    leave(run);
  } finally {
    clearTimeout(timer);
  }
}

module.exports = { resolveShot, renderShots, montageCells, composeMontage, VIEWS, SIZES, SUPERSAMPLE, JPEG_QUALITY, warmRenderer, startRenderer, closeRenderer };
