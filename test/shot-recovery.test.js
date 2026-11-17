'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');

process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS = '1000';
const { drawClient, until } = require('./helpers/draw-client');

test('renderer failures release the queue: a helper that crashes twice, and one that stops answering', async (t) => {
  const renderer = drawClient(t, ['crashes', 'crashes', 'holds', 'healthy']);
  await assert.rejects(renderer.render(), { message: 'Screenshot capture: renderer crashed' });
  assert.equal(renderer.starts().length, 2, 'a crash is retried once');
  await assert.rejects(renderer.render(), /Screenshot capture stalled for 1s; renderer reset, screenshot unavailable/);
  await until(() => !renderer.alive(renderer.starts()[2].pid), 'the helper that stopped answering to be ended');
  const [image] = await renderer.render();
  assert.match(image.toString(), /start 4 /);
});

// A helper that exits between two requests of a call (after the world, before
// the shots) is a crash, as one that exits during a request: the call is
// tried once more on a fresh helper; twice, it fails as a crash does.
test('a helper that exits between two requests of a call is a crash: retried once, then renderer crashed', async (t) => {
  const once = drawClient(t, ['exits after world', 'healthy']);
  const [image] = await once.render();
  assert.match(image.toString(), /start 2 /);
  const twice = drawClient(t, ['exits after world']);
  await assert.rejects(twice.render(), (error) => /^Screenshot (world loading|capture): renderer crashed$/.test(error.message));
  assert.equal(twice.starts().length, 2, 'retried once, not again');
});
