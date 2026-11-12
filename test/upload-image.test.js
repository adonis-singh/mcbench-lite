'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// A canvas that records what is painted on it, in order.
function recordingCanvas() {
  const calls = [];
  const context = {
    set fillStyle(value) { calls.push(['fillStyle', value]); },
    fillRect: (...args) => calls.push(['fillRect', ...args]),
    drawImage: (image, ...args) => calls.push(['drawImage', image.name, ...args]),
  };
  return { calls, canvas: { width: 300, height: 150, getContext: (kind) => { assert.equal(kind, '2d'); return context; } } };
}

test('an upload is painted on white before the image, so transparency is not black', async () => {
  const { paintUpload } = await import('../web/upload-image.mjs');
  const { calls, canvas } = recordingCanvas();
  paintUpload(canvas, { name: 'sprite', width: 64, height: 32 }, 1568);
  assert.deepStrictEqual([canvas.width, canvas.height], [64, 32]);
  assert.deepStrictEqual(calls, [
    ['fillStyle', '#ffffff'],
    ['fillRect', 0, 0, 64, 32],
    ['drawImage', 'sprite', 0, 0, 64, 32],
  ]);
});

test('an upload longer than the edge limit scales down whole, and the white fills all of it', async () => {
  const { paintUpload } = await import('../web/upload-image.mjs');
  const { calls, canvas } = recordingCanvas();
  paintUpload(canvas, { name: 'logo', width: 4000, height: 1000 }, 1568);
  assert.deepStrictEqual([canvas.width, canvas.height], [1568, 392]);
  assert.deepStrictEqual(calls.slice(1), [['fillRect', 0, 0, 1568, 392], ['drawImage', 'logo', 0, 0, 1568, 392]]);
});
