'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// A cache whose requests are settled by hand, in any order.
async function sharedWithRequests() {
  const { Shared } = await import('../web/shared.mjs');
  const requests = [];
  const shared = new Shared((url) => new Promise((resolve, reject) => { requests.push({ url, resolve, reject }); }));
  return { shared, requests };
}

const settled = () => new Promise((resolve) => setImmediate(resolve));

test('a value is fetched once and shared', async () => {
  const { shared, requests } = await sharedWithRequests();
  let notified = 0;
  shared.entry('/a').listeners.add(() => { notified += 1; });
  const first = shared.load('/a');
  assert.equal(shared.load('/a'), first);
  assert.equal(requests.length, 1);
  requests[0].resolve('one');
  assert.equal(await first, 'one');
  assert.deepStrictEqual([shared.entry('/a').value, shared.entry('/a').error, notified], ['one', null, 1]);
  assert.equal(shared.load('/a'), first);
  assert.equal(requests.length, 1);
});

test('a failed request is forgotten, so the next load asks again', async () => {
  const { shared, requests } = await sharedWithRequests();
  const first = shared.load('/a');
  const failure = new Error('HTTP 500');
  requests[0].reject(failure);
  await assert.rejects(first, failure);
  assert.equal(shared.entry('/a').error, failure);
  assert.equal(shared.entry('/a').promise, null);
  const second = shared.load('/a');
  requests[1].resolve('two');
  assert.equal(await second, 'two');
  assert.deepStrictEqual([shared.entry('/a').value, shared.entry('/a').error], ['two', null]);
});

test('a superseded request that succeeds late does not overwrite the newer value', async () => {
  const { shared, requests } = await sharedWithRequests();
  let notified = 0;
  shared.entry('/a').listeners.add(() => { notified += 1; });
  const old = shared.load('/a');
  const fresh = shared.load('/a', { fresh: true });
  assert.notEqual(fresh, old);
  requests[1].resolve('new');
  await fresh;
  requests[0].resolve('old');
  // Whoever asked the old request still gets its answer.
  assert.equal(await old, 'old');
  assert.equal(shared.entry('/a').value, 'new');
  assert.equal(shared.entry('/a').promise, fresh);
  assert.equal(notified, 1);
});

test('a superseded request that fails late leaves the newer request and its value alone', async () => {
  const { shared, requests } = await sharedWithRequests();
  const old = shared.load('/a');
  const fresh = shared.load('/a', { fresh: true });
  // The old one fails while the new one is still on its way.
  requests[0].reject(new Error('old failed'));
  await assert.rejects(old, /old failed/);
  await settled();
  assert.equal(shared.entry('/a').error, null);
  assert.equal(shared.entry('/a').promise, fresh);
  assert.equal(shared.load('/a'), fresh);
  assert.equal(requests.length, 2);
  requests[1].resolve('new');
  await fresh;
  assert.deepStrictEqual([shared.entry('/a').value, shared.entry('/a').error], ['new', null]);
});

test('a superseded request that fails after the newer one landed sets no error', async () => {
  const { shared, requests } = await sharedWithRequests();
  const old = shared.load('/a');
  const fresh = shared.load('/a', { fresh: true });
  requests[1].resolve('new');
  await fresh;
  requests[0].reject(new Error('old failed'));
  await assert.rejects(old, /old failed/);
  assert.deepStrictEqual([shared.entry('/a').value, shared.entry('/a').error, shared.entry('/a').promise], ['new', null, fresh]);
});
