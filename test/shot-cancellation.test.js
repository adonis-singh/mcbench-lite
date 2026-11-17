'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');

const { drawClient, until } = require('./helpers/draw-client');

test('queued screenshots cancel without touching another render; active cancellation stops only the helper\'s job, and the helper stays up', async (t) => {
  const renderer = drawClient(t, ['holds']);
  const first = renderer.render();
  await until(() => renderer.asked('shots').length === 2, 'the first shots held');
  const waiting = new AbortController();
  const second = renderer.render(['front'], waiting.signal);
  waiting.abort();
  await assert.rejects(second, /abort/i);
  renderer.set({ release: 1 });
  assert.equal((await first).length, 1);
  assert.equal(renderer.asked('shots').length, 2, 'the cancelled call never reached the helper');
  assert.equal(renderer.asked('cancel').length, 0);

  const active = new AbortController();
  const third = renderer.render(['front'], active.signal);
  await until(() => renderer.asked('shots').length === 3, 'the third shots held');
  active.abort();
  await assert.rejects(third, /abort/i);
  await until(() => renderer.asked('cancel').length === 1, 'the cancel');
  const fourth = renderer.render();
  await until(() => renderer.asked('shots').length === 4, 'the fourth shots held');
  renderer.set({ release: 3 });
  assert.equal((await fourth).length, 1);
  assert.equal(renderer.starts().length, 1, 'one helper throughout');
});
