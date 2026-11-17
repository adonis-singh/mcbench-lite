#!/usr/bin/env node
'use strict';

// A stand-in for `mcbench-draw serve` (native/draw/PROTOCOL.md) for the
// client's tests (server/shot.js): it speaks the protocol's frames, draws
// nothing, and behaves as the control file FAKE_DRAW_CONTROL says, read at
// every request: { modes: [mode of the 1st start, of the 2nd, ...; the last
// one for every start after], release: n }. It appends what it is asked to
// FAKE_DRAW_LOG, one JSON line each: { start, type, world?, shots?, payloads }.
//
// Modes:
//   healthy        answers every request; a picture is "JPEG" bytes naming the
//                  start, the world and the shot
//   stalls         answers nothing
//   no adapter     hello fails: no graphics adapter
//   no software renderer   hello fails as a helper whose platform ships no
//                  software floor does on a machine with no GPU; it stays up
//   loses device   hello answers; the first shots answers deviceLost, and it exits 3
//   loses device at hello   hello answers deviceLost, and it exits 3
//   crashes        hello answers; shots ends it with status 101 unanswered
//   holds          hello answers; shots answer only once `release` in the
//                  control file reaches their number (1 for the first shots,
//                  2 for the second...), or when cancelled
//   never exits    healthy, but stays up when its stdin ends
//   exits after world  answers a world, then exits (between two requests of a call)
//   forgets bases  healthy, but answers every world naming a base as if the
//                  base were evicted (unknownWorld)

const fs = require('fs');

const control = () => JSON.parse(fs.readFileSync(process.env.FAKE_DRAW_CONTROL, 'utf8'));
const log = (entry) => fs.appendFileSync(process.env.FAKE_DRAW_LOG, `${JSON.stringify({ start, ...entry })}\n`);
const start = fs.existsSync(process.env.FAKE_DRAW_LOG) ? fs.readFileSync(process.env.FAKE_DRAW_LOG, 'utf8').split('\n').filter((line) => line && JSON.parse(line).type === 'start').length + 1 : 1;
const { modes } = control();
const mode = modes[Math.min(start, modes.length) - 1];
log({ type: 'start', mode, pid: process.pid });

const kept = new Set();
const held = new Map();
let shotsAsked = 0;
let said = false;

function send(header, payloads = []) {
  const json = Buffer.from(JSON.stringify(payloads.length ? { ...header, payloads: payloads.map((p) => p.length) } : header));
  const length = Buffer.alloc(4);
  length.writeUInt32LE(json.length);
  process.stdout.write(Buffer.concat([length, json, ...payloads]));
}

const picture = (text) => Buffer.concat([Buffer.from([0xff, 0xd8, 0xff]), Buffer.from(text)]);
const shotTimings = { scene: 1, draw: 2, readback: 3, grid: 0, encode: 4, total: 10 };

function shotsReply(header) {
  return [{ id: header.id, ok: true, shots: header.shots.map((s) => ({ width: s.width, height: s.height, bytes: 1, timings: shotTimings })) },
    header.shots.map((_, i) => picture(` start ${start} ${header.world} shot ${i}`))];
}

function handle(header, payloads) {
  log({ type: header.type, world: header.world, base: header.base, keepBase: header.keepBase, memoryLimit: header.memoryLimit, shots: header.shots?.length, payloads: payloads.length });
  if (mode === 'stalls') return;
  if (header.type === 'hello') {
    if (mode === 'no adapter') return send({ id: header.id, error: 'No graphics adapter: metal: none found' });
    if (mode === 'no software renderer') return send({ id: header.id, error: 'This machine has no graphics adapter, and this build for Linux aarch64 ships no software renderer' });
    if (mode === 'loses device at hello') {
      send({ id: header.id, error: 'The graphics device was lost: fixture', deviceLost: true });
      return process.exit(3);
    }
    said = true;
    return send({ id: header.id, ok: true, version: 'mcbench-draw 0.0.0 fake', adapter: { name: 'Fixture GPU', backend: 'metal', deviceType: 'IntegratedGpu', driver: '', driverInfo: '', software: false },
      limits: { worldsKept: 4, scenesKept: 6 }, timings: { device: 1, assets: 1, pipelines: 1, total: 3 } });
  }
  if (!said && header.type !== 'montage') return send({ id: header.id, error: 'The helper has not been given its assets (hello) yet' });
  if (header.type === 'world') {
    if (header.base !== undefined && (!kept.has(header.base) || mode === 'forgets bases')) return send({ id: header.id, error: `No world named ${header.base} is kept`, unknownWorld: true });
    if (header.base !== undefined && !header.keepBase) kept.delete(header.base);
    kept.add(header.world);
    if (mode === 'exits after world') {
      send({ id: header.id, ok: true, quads: 1, entityQuads: 0, tiles: 1, blocks: 1, missingTextures: [], timings: { read: 1, mesh: 1, entities: 0, upload: 1, total: 3 } });
      return process.exit(1);
    }
    if (header.base !== undefined) return send({ id: header.id, ok: true, patched: !header.keepBase, tilesMeshed: 1, quads: 1, entityQuads: 0, tiles: 1, blocks: 1, missingTextures: [], timings: { read: 1, mesh: 1, entities: 0, upload: 1, total: 3 } });
    return send({ id: header.id, ok: true, quads: 1, entityQuads: 0, tiles: 1, blocks: 1, missingTextures: [], timings: { read: 1, mesh: 2, entities: 0, upload: 1, total: 4 } });
  }
  if (header.type === 'drop') return send({ id: header.id, ok: true, dropped: kept.delete(header.world) });
  if (header.type === 'montage') return send({ id: header.id, ok: true, timings: { decode: 1, encode: 1, total: 2 } }, [picture(` card of ${payloads.length}`)]);
  if (header.type === 'cancel') {
    const job = held.get(header.job);
    send({ id: header.id, ok: true, found: Boolean(job) });
    if (job) {
      held.delete(header.job);
      send({ id: header.job, error: 'cancelled' });
    }
    return;
  }
  if (header.type === 'shots') {
    if (!kept.has(header.world)) return send({ id: header.id, error: `No world named ${header.world} is kept`, unknownWorld: true });
    shotsAsked += 1;
    if (mode === 'loses device' && shotsAsked === 1) {
      send({ id: header.id, error: 'The graphics device was lost: fixture', deviceLost: true });
      return process.exit(3);
    }
    if (mode === 'crashes') return process.exit(101);
    if (mode === 'holds') return held.set(header.id, { number: shotsAsked, header });
    return send(...shotsReply(header));
  }
  return send({ id: header.id, error: `Unknown request type ${header.type}` });
}

// Held shots answer once released.
setInterval(() => {
  if (!held.size) return;
  const { release = 0 } = control();
  for (const [id, job] of held) {
    if (job.number <= release) {
      held.delete(id);
      send(...shotsReply(job.header));
    }
  }
}, 5);

let buffer = Buffer.alloc(0);
process.stdin.on('data', (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    if (buffer.length < 4) return;
    const n = buffer.readUInt32LE(0);
    if (buffer.length < 4 + n) return;
    const header = JSON.parse(buffer.subarray(4, 4 + n).toString('utf8'));
    const lengths = header.payloads || [];
    const total = 4 + n + lengths.reduce((a, b) => a + b, 0);
    if (buffer.length < total) return;
    const payloads = [];
    let at = 4 + n;
    for (const length of lengths) { payloads.push(buffer.subarray(at, at + length)); at += length; }
    buffer = buffer.subarray(total);
    handle(header, payloads);
  }
});
process.stdin.on('end', () => {
  if (mode !== 'never exits' && mode !== 'stalls') process.exit(0);
});
