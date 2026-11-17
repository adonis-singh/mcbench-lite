'use strict';

// A lost graphics device and a helper that exited, with mcbench-draw faked
// (test/helpers/fake-draw.js): the call is tried once more on a fresh helper,
// which is sent the world again; a closed renderer stays closed.

const { test } = require('node:test');
const assert = require('node:assert/strict');

const { drawClient, until } = require('./helpers/draw-client');

test('a lost device sends the world to a fresh helper and retries the whole inspection once; lost twice, the call fails', async (t) => {
  const renderer = drawClient(t, ['loses device', 'healthy']);
  const images = await renderer.render(['front', 'top']);
  assert.deepEqual(images.map((image) => image.subarray(3).toString().trim()), ['start 2 fixture/tick 0/1 shot 0', 'start 2 fixture/tick 0/1 shot 1']);
  assert.deepEqual(renderer.asked('world').map((entry) => entry.start), [1, 2], 'the world is sent to each helper once');
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the helper that lost its device to exit');

  // A closed renderer stays closed; a fresh one starts from no helper.
  await renderer.closeRenderer();
  await assert.rejects(renderer.render(), /renderer was shut down/);
  const fresh = drawClient(t, ['loses device']);
  await assert.rejects(fresh.render(['front', 'top']), { message: 'page.evaluate: Error: Screenshot WebGL context lost' });
  assert.equal(fresh.starts().length, 2, 'a lost device is retried once, not again');
});

test('a helper that exited is started again, and a render queued behind closeRenderer starts none', async (t) => {
  const renderer = drawClient(t, ['healthy', 'holds']);
  await renderer.render();
  process.kill(renderer.starts()[0].pid, 'SIGKILL');
  await until(() => !renderer.alive(renderer.starts()[0].pid), 'the helper to die');
  // The second helper holds its shots until released.
  const running = renderer.render();
  const queued = renderer.render();
  await until(() => renderer.asked('shots').filter((entry) => entry.start === 2).length === 2, 'the held shots');
  const closing = renderer.closeRenderer();
  renderer.set({ release: 1 });
  assert.equal((await running).length, 1);
  await assert.rejects(queued, /renderer was shut down/);
  await closing;
  assert.equal(renderer.starts().length, 2);
});

test('a world is sent once and drawn again from the helper\'s copy; one the helper no longer keeps is sent again', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  const world = renderer.world();
  await renderer.render(['front'], undefined, world);
  await renderer.render(['top', 'front'], undefined, world);
  assert.equal(renderer.asked('world').length, 1);
  assert.deepEqual(renderer.asked('shots').map((entry) => entry.shots), [1, 1, 2], 'the first shots found no world; then each call is one request');
  // Another settled world is another world to the helper.
  await renderer.render(['front'], undefined, renderer.world());
  assert.equal(renderer.asked('world').length, 2);
});

// Incremental worlds (PROTOCOL.md `base`, `keepBase`): an edit names the
// conversation's previous world of the same tick and lets the helper drop
// it; another tick of the same build keeps its base; other conversations'
// worlds are never bases; a base the helper no longer holds is sent whole.
test('an edit of the build names the previous world as its base and drops it', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  const before = renderer.world('e1');
  const after = renderer.world('e2');
  await renderer.renderOf('chat', [[before, 210]]);
  await renderer.renderOf('chat', [[after, 210]]);
  const worlds = renderer.asked('world');
  assert.deepEqual(worlds.map((entry) => [entry.world, entry.base ?? null, entry.keepBase ?? null]), [
    ['chat/tick 210/1', null, null],
    ['chat/tick 210/2', 'chat/tick 210/1', false],
  ]);
  // The dropped world is no one's base again: the next edition patches the last one.
  await renderer.renderOf('chat', [[renderer.world('e3'), 210]]);
  assert.equal(renderer.asked('world').at(-1).base, 'chat/tick 210/2');
});

test('another tick of the same build keeps its base, and a later call draws both without sending either', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  const settled = renderer.world('e1');
  const ticked = renderer.world('e1');
  await renderer.renderOf('chat', [[settled, 210], [ticked, 20]]);
  assert.deepEqual(renderer.asked('world').map((entry) => [entry.world, entry.base ?? null, entry.keepBase ?? null]), [
    ['chat/tick 210/1', null, null],
    ['chat/tick 20/2', 'chat/tick 210/1', true],
  ]);
  await renderer.renderOf('chat', [[settled, 210], [ticked, 20]]);
  assert.equal(renderer.asked('world').length, 2, 'both are kept');
});

test('worlds of other conversations are never bases', async (t) => {
  const renderer = drawClient(t, ['healthy']);
  await renderer.renderOf('one', [[renderer.world('a'), 210]]);
  await renderer.renderOf('two', [[renderer.world('b'), 210]]);
  assert.deepEqual(renderer.asked('world').map((entry) => entry.base ?? null), [null, null]);
});

test('a base the helper no longer holds is answered unknownWorld, and the world is sent whole', async (t) => {
  const renderer = drawClient(t, ['forgets bases']);
  await renderer.renderOf('chat', [[renderer.world('e1'), 210]]);
  const [image] = await renderer.renderOf('chat', [[renderer.world('e2'), 210]]);
  assert.match(image.toString(), /chat\/tick 210\/2 shot 0/);
  assert.deepEqual(renderer.asked('world').map((entry) => [entry.world, entry.base ?? null]), [
    ['chat/tick 210/1', null],
    ['chat/tick 210/2', 'chat/tick 210/1'],
    ['chat/tick 210/2', null],
  ]);
});
