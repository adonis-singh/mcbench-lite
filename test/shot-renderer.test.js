'use strict';

// The renderer's states, with mcbench-draw faked (test/helpers/fake-draw.js):
// a start is bounded like every other stage, a lost device or a crash
// starts the helper once more, and a run that ends for any reason ends its
// helper.

const { test } = require('node:test');
const assert = require('node:assert/strict');

process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS = '1000';
const { drawClient, within, until } = require('./helpers/draw-client');

test('a start that stalls fails like any stage, ends its helper, and lets the next screenshot through', async (t) => {
  const renderer = drawClient(t, ['stalls', 'healthy']);
  const warming = renderer.warmRenderer();
  const queued = renderer.render();
  const outcome = await within(warming, 5000);
  assert.match(String(outcome.message), /^Screenshot initialization stalled for 1s; renderer reset, screenshot unavailable$/);
  assert.equal(await within(queued, 5000), 'resolved');
  const [stalled, second] = renderer.starts();
  assert.equal(renderer.starts().length, 2, 'on a helper of its own');
  assert.equal(renderer.alive(stalled.pid), false, 'the stalled helper was ended');
  await renderer.closeRenderer();
  await until(() => !renderer.alive(second.pid), 'the second helper to exit');
});

test('a start whose device is lost starts once more on a fresh helper', async (t) => {
  const renderer = drawClient(t, ['loses device at hello', 'healthy']);
  await renderer.warmRenderer();
  assert.deepEqual(renderer.starts().map((start) => start.mode), ['loses device at hello', 'healthy'], 'exactly one more helper');
  assert.equal((await renderer.render()).length, 1);
  assert.equal(renderer.starts().length, 2);
});

test('a start whose device is lost twice fails with the lost device', async (t) => {
  const renderer = drawClient(t, ['loses device at hello']);
  await assert.rejects(renderer.warmRenderer(), { message: 'page.evaluate: Error: Screenshot WebGL context lost' });
  assert.equal(renderer.starts().length, 2, 'no third helper');
});

test('closeRenderer ends although a start never answers', async (t) => {
  const renderer = drawClient(t, ['stalls']);
  const warming = renderer.warmRenderer();
  warming.catch(() => {});
  await until(() => renderer.starts().length === 1, 'the helper to start');
  assert.equal(await within(renderer.closeRenderer(), 5000), 'resolved');
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the helper to be ended');
});

test('a helper that exits with no screenshot under way is started again by the next one', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  await renderer.render();
  const [first] = renderer.starts();
  process.kill(first.pid, 'SIGKILL');
  await until(() => !renderer.alive(first.pid), 'the helper to die');
  const [image] = await renderer.render();
  assert.match(image.toString(), /start 2 /);
  assert.equal(renderer.starts().length, 2);
});

test('a helper that finds no graphics adapter fails the call saying so, and the next call starts another', async (t) => {
  const renderer = drawClient(t, ['no adapter', 'healthy']);
  await assert.rejects(renderer.render(), { message: 'Screenshot initialization: No graphics adapter: metal: none found' });
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the failed helper to be ended');
  assert.equal((await renderer.render()).length, 1);
  assert.equal(renderer.starts().length, 2);
});

test('closeRenderer fails, within a stage\'s time, on a helper that does not exit, and kills it', async (t) => {
  const renderer = drawClient(t, ['never exits']);
  await renderer.render();
  const outcome = await within(renderer.closeRenderer(), 5000);
  assert.match(String(outcome.message), /The screenshot renderer did not close within 1s/);
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the helper to be killed');
  await assert.rejects(renderer.render(), /The screenshot renderer was shut down/);
  assert.equal(renderer.starts().length, 1, 'and no helper starts again');
});

// A platform whose build ships no software renderer (linux-arm64), on a
// machine with no GPU: the helper says so at hello. That is the first
// start's step `draw` failing with the helper's sentence, and each see_build
// after it answering the same sentence, with one helper started per call and
// none in between.
test('a helper that cannot draw on this machine fails the setup step and see_build with its sentence, and is not started again on its own', async (t) => {
  const setup = require('../server/setup');
  setup.reset();
  t.after(() => setup.reset());
  t.mock.method(require('../server/minecraft-live').minecraftLive, 'prepareViewer', async () => {});
  const renderer = drawClient(t, ['no software renderer']);
  const sentence = /This machine has no graphics adapter, and this build for Linux aarch64 ships no software renderer/;
  await assert.rejects(renderer.startRenderer(), sentence);
  const step = setup.view().steps.find((entry) => entry.id === 'draw');
  assert.equal(step.state, 'failed');
  assert.match(step.error, sentence);
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the helper to be ended');
  await new Promise((resolve) => setTimeout(resolve, 300));
  assert.equal(renderer.starts().length, 1, 'no restart on its own');
  await assert.rejects(renderer.render(), sentence);
  assert.equal(renderer.starts().length, 2, 'one start for the call, no retry');
});

test('hello leaves the bound on what the helper keeps between calls to the helper', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  await renderer.render();
  assert.deepEqual(renderer.asked('hello').map((entry) => entry.memoryLimit), [undefined]);
});
