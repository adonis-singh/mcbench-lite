'use strict';

// The frames of mcbench-draw's protocol (native/draw/PROTOCOL.md), as the
// server reads them (server/draw.js FrameReader).

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { encodeFrame, FrameReader } = require('../server/draw');

const read = (chunks) => {
  const frames = [];
  const reader = new FrameReader((header, payloads) => frames.push({ header, payloads }));
  for (const chunk of chunks) reader.push(chunk);
  return frames;
};

test('frames split anywhere, and several in one chunk, are read whole and in order', () => {
  const bytes = Buffer.concat([
    encodeFrame({ id: 1, ok: true }),
    encodeFrame({ id: 2, ok: true }, [Buffer.from('one'), Buffer.from(''), Buffer.from('three')]),
    encodeFrame({ id: 3, error: 'no' }),
  ]);
  for (const size of [1, 2, 3, 5, 7, bytes.length]) {
    const chunks = [];
    for (let at = 0; at < bytes.length; at += size) chunks.push(bytes.subarray(at, at + size));
    const frames = read(chunks);
    assert.deepEqual(frames.map((frame) => frame.header.id), [1, 2, 3], `chunks of ${size}`);
    assert.deepEqual(frames[1].payloads.map(String), ['one', '', 'three']);
  }
});

// A 50 MB reply arriving in 64 KiB chunks, as a pipe gives it: read once,
// so it takes about as long as copying it a few times, not once per chunk
// (re-joining the whole buffer for each of 800 chunks copied 20 GB).
test('a 50 MB reply in 64 KiB chunks is read in linear time', () => {
  const payload = Buffer.alloc(50 * 1024 * 1024, 7);
  const bytes = encodeFrame({ id: 1, ok: true }, [payload]);
  const chunks = [];
  for (let at = 0; at < bytes.length; at += 65536) chunks.push(bytes.subarray(at, at + 65536));
  const started = process.hrtime.bigint();
  const [frame] = read(chunks);
  const ms = Number(process.hrtime.bigint() - started) / 1e6;
  assert.equal(frame.payloads[0].length, payload.length);
  assert.ok(frame.payloads[0].equals(payload));
  assert.ok(ms < 1000, `read in ${ms.toFixed(0)} ms`);
});
