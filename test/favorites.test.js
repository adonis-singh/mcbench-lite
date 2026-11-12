'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// Favorites whose saves are settled by hand. `writes` holds each list as it
// was sent; `shown` each list the page was given.
async function favorites() {
  const { Favorites } = await import('../web/favorites.mjs');
  const writes = [];
  const shown = [];
  const errors = [];
  const sync = new Favorites({
    write: (list) => new Promise((resolve, reject) => { writes.push({ list, resolve, reject }); }),
    onChange: (list) => shown.push(list),
    onError: (error) => errors.push(error.message),
  });
  return { sync, writes, shown, errors };
}

const settled = () => new Promise((resolve) => setImmediate(resolve));

test('nothing is starred, and nothing saved, before the server\'s list is known', async () => {
  const { sync, writes, shown } = await favorites();
  assert.throws(() => sync.toggle('p::m'), /have not loaded/);
  assert.deepStrictEqual(writes, []);
  assert.deepStrictEqual(shown, []);
  sync.load(['p::a', 'p::b']);
  assert.deepStrictEqual(shown, [['p::a', 'p::b']]);
  sync.toggle('p::m');
  // The save is the server's list with the star on it, not this page's alone.
  assert.deepStrictEqual(writes.map((write) => write.list), [['p::a', 'p::b', 'p::m']]);
  assert.throws(() => sync.load([]), /have loaded already/);
});

test('a star that could not be saved goes back, and says so', async () => {
  const { sync, writes, shown, errors } = await favorites();
  sync.load(['p::a']);
  sync.toggle('p::m');
  assert.deepStrictEqual(sync.list, ['p::a', 'p::m']);
  writes[0].reject(new Error('disk full'));
  await settled();
  assert.deepStrictEqual(sync.list, ['p::a']);
  assert.deepStrictEqual(shown.at(-1), ['p::a']);
  assert.deepStrictEqual(errors, ['disk full']);
  assert.equal(sync.phase, 'idle');
  // Taking a star off that could not be saved puts it back on.
  sync.toggle('p::a');
  assert.deepStrictEqual(sync.list, []);
  writes[1].reject(new Error('disk full'));
  await settled();
  assert.deepStrictEqual(sync.list, ['p::a']);
});

test('stars are saved one at a time, in order, and a failed one takes no other with it', async () => {
  const { sync, writes, errors } = await favorites();
  sync.load([]);
  sync.toggle('p::a');
  sync.toggle('p::b');
  sync.toggle('p::c');
  assert.deepStrictEqual(sync.list, ['p::a', 'p::b', 'p::c']);
  assert.equal(writes.length, 1);
  writes[0].resolve(['p::a']);
  await settled();
  // The second save fails: its star goes back, and the third is sent
  // without it.
  assert.deepStrictEqual(writes[1].list, ['p::a', 'p::b']);
  writes[1].reject(new Error('refused'));
  await settled();
  assert.deepStrictEqual(sync.list, ['p::a', 'p::c']);
  assert.deepStrictEqual(writes[2].list, ['p::a', 'p::c']);
  writes[2].resolve(['p::a', 'p::c']);
  await settled();
  assert.deepStrictEqual(sync.list, ['p::a', 'p::c']);
  assert.deepStrictEqual(errors, ['refused']);
  assert.equal(sync.phase, 'idle');
  assert.equal(writes.length, 3);
});
