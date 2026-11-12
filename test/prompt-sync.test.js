'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// A library whose reads and writes are settled by hand. `writes` holds each
// list as it was sent.
async function library() {
  const { PromptSync } = await import('../web/prompt-sync.mjs');
  const reads = [];
  const writes = [];
  const phases = [];
  const errors = [];
  const pending = (list) => new Promise((resolve, reject) => { list.push({ resolve, reject }); });
  const sync = new PromptSync({
    read: () => pending(reads),
    write: (list) => { const sent = pending(writes); writes.at(-1).list = list; return sent; },
    onChange: (state) => phases.push(state.phase),
    onError: (error) => errors.push(error.message),
    delay: 0,
  });
  return { sync, reads, writes, phases, errors };
}

// Past the save timer and every promise reaction.
const settled = async () => {
  await new Promise((resolve) => setTimeout(resolve, 5));
  await new Promise((resolve) => setImmediate(resolve));
};

const add = (id) => (list) => [...list, { id }];
const ids = (list) => list.map((prompt) => prompt.id);

test('no edit is taken, and nothing is saved, before the library has loaded', async () => {
  const { sync, reads, writes, phases, errors } = await library();
  assert.equal(sync.phase, 'unloaded');
  assert.throws(() => sync.edit(add('early')), /has not loaded/);
  sync.load();
  assert.equal(sync.phase, 'loading');
  assert.throws(() => sync.edit(add('early')), /has not loaded/);
  reads[0].reject(new Error('HTTP 500'));
  await settled();
  assert.equal(sync.phase, 'failed');
  assert.equal(sync.snapshot().error.message, 'HTTP 500');
  assert.deepStrictEqual(errors, ['HTTP 500']);
  assert.throws(() => sync.edit(add('early')), /has not loaded/);
  await settled();
  // The refused edits are not queued against the empty list.
  assert.deepStrictEqual([sync.list, sync.unsent, writes], [[], [], []]);

  // The failed load is read again, and then edits are taken and saved.
  sync.load();
  reads[1].resolve([{ id: 'a' }]);
  await settled();
  assert.equal(sync.phase, 'ready');
  assert.equal(sync.snapshot().error, null);
  sync.edit(add('b'));
  await settled();
  assert.equal(sync.phase, 'saving');
  assert.deepStrictEqual(ids(writes[0].list), ['a', 'b']);
  writes[0].resolve();
  await settled();
  assert.deepStrictEqual([sync.phase, sync.unsent.length, writes.length], ['ready', 0, 1]);
  assert.deepStrictEqual(phases, ['loading', 'failed', 'loading', 'ready', 'ready', 'saving', 'ready']);
});

test('a load cannot start while one is on its way or once it has landed', async () => {
  const { sync, reads } = await library();
  sync.load();
  assert.throws(() => sync.load(), /cannot go from loading to loading/);
  reads[0].resolve([]);
  await settled();
  assert.throws(() => sync.load(), /cannot go from ready to loading/);
});

test('a failed save keeps its edits, which go again with the next one', async () => {
  const { sync, reads, writes, errors } = await library();
  sync.load();
  reads[0].resolve([{ id: 'a' }]);
  await settled();
  sync.edit(add('b'));
  await settled();
  writes[0].reject(new Error('disk full'));
  await settled();
  assert.deepStrictEqual([sync.phase, sync.unsent.length, errors], ['ready', 1, ['disk full']]);
  // It is not sent again by itself.
  await settled();
  assert.equal(writes.length, 1);

  // A reload meanwhile lands the server's list with the unsent edit on it.
  sync.reload();
  assert.equal(sync.phase, 'reloading');
  reads[1].resolve([{ id: 'a' }, { id: 'builder' }]);
  await settled();
  assert.deepStrictEqual(ids(sync.list), ['a', 'builder', 'b']);
  // And sends what the failed save held.
  assert.equal(sync.phase, 'saving');
  assert.deepStrictEqual(ids(writes[1].list), ['a', 'builder', 'b']);
  writes[1].reject(new Error('disk full'));
  await settled();
  assert.equal(sync.unsent.length, 1);

  // The next edit's save carries both.
  sync.edit(add('c'));
  await settled();
  assert.deepStrictEqual(ids(writes[2].list), ['a', 'builder', 'b', 'c']);
  writes[2].resolve();
  await settled();
  assert.deepStrictEqual([sync.phase, sync.unsent.length], ['ready', 0]);
  await settled();
  assert.equal(writes.length, 3);
});

test('an edit made while a save is on its way stays unsent until its own save', async () => {
  const { sync, reads, writes } = await library();
  sync.load();
  reads[0].resolve([]);
  await settled();
  sync.edit(add('a'));
  await settled();
  assert.equal(sync.phase, 'saving');
  sync.edit(add('b'));
  assert.deepStrictEqual(ids(writes[0].list), ['a']);
  writes[0].resolve();
  await settled();
  // Only the edit the save carried is cleared; the other goes next.
  assert.equal(sync.phase, 'saving');
  assert.equal(sync.unsent.length, 1);
  assert.deepStrictEqual(ids(writes[1].list), ['a', 'b']);
  writes[1].resolve();
  await settled();
  assert.deepStrictEqual([sync.phase, sync.unsent.length], ['ready', 0]);
});

test('edits made close together are saved once', async () => {
  const { sync, reads, writes } = await library();
  sync.load();
  reads[0].resolve([]);
  await settled();
  sync.edit(add('a'));
  sync.edit(add('b'));
  sync.edit(add('c'));
  await settled();
  assert.equal(writes.length, 1);
  assert.deepStrictEqual(ids(writes[0].list), ['a', 'b', 'c']);
});

test('a reload waits for the save on its way, and an edit made during it is kept', async () => {
  const { sync, reads, writes } = await library();
  sync.load();
  reads[0].resolve([{ id: 'a' }]);
  await settled();
  sync.edit(add('b'));
  await settled();
  sync.reload();
  assert.equal(sync.phase, 'saving');
  assert.equal(reads.length, 1);
  writes[0].resolve();
  await settled();
  assert.equal(sync.phase, 'reloading');
  sync.edit(add('c'));
  assert.deepStrictEqual(ids(sync.list), ['a', 'b', 'c']);
  // Another change on the server while this read is out: it is read again.
  sync.reload();
  reads[1].resolve([{ id: 'a' }, { id: 'b' }, { id: 'x' }]);
  await settled();
  assert.equal(sync.phase, 'reloading');
  assert.equal(writes.length, 1);
  reads[2].resolve([{ id: 'a' }, { id: 'b' }, { id: 'x' }, { id: 'y' }]);
  await settled();
  assert.deepStrictEqual(ids(sync.list), ['a', 'b', 'x', 'y', 'c']);
  assert.equal(sync.phase, 'saving');
  assert.deepStrictEqual(ids(writes[1].list), ['a', 'b', 'x', 'y', 'c']);
});

test('a reload asked for while the first read is out follows it', async () => {
  const { sync, reads } = await library();
  sync.load();
  sync.reload();
  reads[0].resolve([{ id: 'old' }]);
  await settled();
  assert.equal(sync.phase, 'reloading');
  reads[1].resolve([{ id: 'new' }]);
  await settled();
  assert.deepStrictEqual([sync.phase, ids(sync.list)], ['ready', ['new']]);
});

test('a failed reload leaves the list stale: it is read again before anything is saved over the server', async () => {
  const { sync, reads, writes, errors } = await library();
  sync.load();
  reads[0].resolve([{ id: 'a' }]);
  await settled();
  sync.reload();
  reads[1].reject(new Error('offline'));
  await settled();
  assert.deepStrictEqual([sync.phase, errors, ids(sync.list)], ['ready', ['offline'], ['a']]);
  sync.edit(add('b'));
  assert.equal(sync.phase, 'reloading');
  await settled();
  assert.equal(writes.length, 0);
  reads[2].resolve([{ id: 'a' }, { id: 'builder' }]);
  await settled();
  assert.equal(sync.phase, 'saving');
  assert.deepStrictEqual(ids(writes[0].list), ['a', 'builder', 'b']);
});
