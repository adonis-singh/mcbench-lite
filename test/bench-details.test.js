'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const data = { rounds: [{ id: 'r1' }], stats: {}, votes: {} };

test('bench details are loading, loaded or failed, and a failed read can be made again', async () => {
  const { detailsReading, detailsRead, detailsFailed } = await import('../web/bench-details.mjs');
  let details = detailsReading(undefined);
  assert.deepStrictEqual(details, { phase: 'loading', data: null, error: null });
  details = detailsFailed(details, new Error('HTTP 500'));
  assert.deepStrictEqual(details, { phase: 'failed', data: null, error: 'HTTP 500' });
  // Nothing lands on a failed read: it is read again first.
  assert.throws(() => detailsRead(details, data), /cannot go from failed to loaded/);
  details = detailsReading(details);
  assert.equal(details.phase, 'loading');
  details = detailsRead(details, data);
  assert.deepStrictEqual(details, { phase: 'loaded', data, error: null });
  assert.throws(() => detailsFailed(details, new Error('late')), /cannot go from loaded to failed/);
  assert.throws(() => detailsRead(undefined, data), /only for a read that was begun/);
});

test('what was read stays on show through a later read, and one that fails', async () => {
  const { detailsReading, detailsRead, detailsFailed, detailsVotes } = await import('../web/bench-details.mjs');
  let details = detailsRead(detailsReading(undefined), data);
  details = detailsReading(details);
  assert.deepStrictEqual(details, { phase: 'loading', data, error: null });
  // A vote cast meanwhile shows at once.
  details = detailsVotes(details, { r1: { pick: 'm1' } });
  assert.deepStrictEqual(details.data.votes, { r1: { pick: 'm1' } });
  details = detailsFailed(details, new Error('offline'));
  assert.equal(details.phase, 'failed');
  assert.equal(details.error, 'offline');
  assert.deepStrictEqual(details.data.rounds, data.rounds);
  assert.throws(() => detailsVotes(detailsReading(undefined), {}), /never read hold no votes/);
});
