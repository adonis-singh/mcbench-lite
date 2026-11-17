'use strict';

// A helper with nothing to do is stopped after the idle time and started
// again by the next call; a call queued or under way keeps it (mcbench-draw
// faked: test/helpers/fake-draw.js).

const { test } = require('node:test');
const assert = require('node:assert/strict');

process.env.MCBENCH_SHOT_IDLE_MS = '200';
const { drawClient, pause, until } = require('./helpers/draw-client');

test('a helper idle for the idle time is stopped, and the next call starts another', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  await renderer.render();
  const [first] = renderer.starts();
  await pause(80);
  assert.equal(renderer.alive(first.pid), true, 'not before the idle time');
  await until(() => !renderer.alive(first.pid), 'the idle helper to be stopped', 3000);
  const [image] = await renderer.render();
  assert.match(image.toString(), /start 2 /);
  assert.equal(renderer.starts().length, 2);
});

test('a call under way or queued keeps the helper past the idle time', async (t) => {
  const renderer = drawClient(t, ['holds']);
  const running = renderer.render();
  const queued = renderer.render();
  await until(() => renderer.asked('shots').length === 2, 'the held shots');
  await pause(500);
  const [helper] = renderer.starts();
  assert.equal(renderer.alive(helper.pid), true, 'held through 2.5 idle times');
  renderer.set({ release: 3 });
  await running;
  await queued;
  assert.equal(renderer.starts().length, 1);
  await until(() => !renderer.alive(helper.pid), 'the helper to be stopped once idle', 3000);
});

test('a call that comes within the idle time keeps the same helper', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  for (let i = 0; i < 4; i += 1) {
    await renderer.render();
    await pause(100);
  }
  assert.equal(renderer.starts().length, 1);
});
