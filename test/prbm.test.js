'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { writePrbm, expand } = require('./helpers/prbm');

test('quad corners are shared only when identical, and the drawn corners are unchanged', async () => {
  const { shareQuadCorners } = await import('./fixtures/prbm-reference.mjs');
  // Two quads as (a, b, c)(a, c, d), then a pair whose "shared" corner differs
  // in one attribute, then a lone triangle; two material groups.
  const quad = (x, light) => [[x, 0, 0, light], [x + 1, 0, 0, light], [x + 1, 1, 0, light], [x, 0, 0, light], [x + 1, 1, 0, light], [x, 1, 0, light]];
  const corners = [...quad(0, 15), ...quad(2, 7), ...quad(4, 3).map((c, i) => (i === 3 ? [c[0], c[1], c[2], 9] : c)), [9, 9, 9, 1], [9, 8, 9, 1], [8, 9, 9, 1]];
  const position = new Float32Array(corners.flatMap(([x, y, z]) => [x, y, z]));
  const light = new Int8Array(corners.map((c) => c[3]));
  const color = new Uint8Array(corners.flatMap((c, i) => [i % 7, 3, 9]));
  for (let i = 0; i < corners.length; i++) if ([0, 3, 2, 4, 6, 9, 8, 10].includes(i)) color.set([0, 3, 9], i * 3); // shared corners agree
  const original = writePrbm([
    { name: 'position', flags: 0x20 | 1, values: position },
    { name: 'sunlight', flags: 0x00 | 3, values: light },
    { name: 'color', flags: 0x40 | 0x20 | 7, values: color },
  ], corners.length, [{ materialIndex: 0, start: 0, count: 12 }, { materialIndex: 3, start: 12, count: 9 }]);

  const indexed = shareQuadCorners(new Uint8Array(original));
  assert.equal(indexed.byteOffset, 0);
  assert.equal(indexed.buffer.byteLength, indexed.length, 'the buffer holds exactly the tile');
  const before = expand(original);
  const after = expand(Buffer.from(indexed));
  assert.deepEqual(after.corners, before.corners, 'every drawn corner keeps its exact bytes and order');
  assert.equal(after.groups, before.groups, 'material groups are unchanged');
  assert.ok(after.unique < before.unique, `fewer vertices to shade (${after.unique} of ${before.unique})`);
  assert.equal(after.unique, 4 + 4 + 6 + 3, 'two quads shared, the mismatched pair and the lone triangle kept');
  assert.throws(() => shareQuadCorners(indexed), /indexed already/);
  assert.throws(() => shareQuadCorners(new Uint8Array(original.subarray(0, 20))), /Malformed PRBM/);
});
